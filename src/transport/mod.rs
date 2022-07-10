use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use basedrop::{Shared, SharedCell};
use meadowlark_core_types::{Frames, MusicalTime, SampleRate};

mod tempo_map;

pub use tempo_map::TempoMap;

use crate::plugin::events::{
    EventBeatTime, EventFlags, EventSecTime, EventTransport, TransportFlags,
};
use crate::ProcEvent;

pub struct TransportSaveState {
    pub seek_to: MusicalTime,
    pub loop_state: LoopState,
}

impl Default for TransportSaveState {
    fn default() -> Self {
        Self { seek_to: MusicalTime::new(0, 0), loop_state: LoopState::Inactive }
    }
}

pub struct TransportHandle {
    parameters: Shared<SharedCell<Parameters>>,

    tempo_map_shared: Shared<SharedCell<(Shared<TempoMap>, u64)>>,

    playhead_frame_shared: Arc<AtomicU64>,
    playhead_frame: Frames,
    playhead_musical: MusicalTime,

    seek_to: MusicalTime,

    coll_handle: basedrop::Handle,
}

impl TransportHandle {
    pub fn seek_to(&mut self, seek_to: MusicalTime) {
        self.seek_to = seek_to;

        let mut params = Parameters::clone(&self.parameters.get());
        params.seek_to = (seek_to, params.seek_to.1 + 1);
        self.parameters.set(Shared::new(&self.coll_handle, params));
    }

    pub fn set_playing(&mut self, playing: bool) {
        let mut params = Parameters::clone(&self.parameters.get());
        params.is_playing = playing;
        self.parameters.set(Shared::new(&self.coll_handle, params));
    }

    /// Set the looping state.
    pub fn set_loop_state(&mut self, loop_state: LoopState) {
        let mut params = Parameters::clone(&self.parameters.get());
        params.loop_state = (loop_state, params.loop_state.1 + 1);
        self.parameters.set(Shared::new(&self.coll_handle, params));
    }

    pub fn playhead_position(&mut self) -> MusicalTime {
        let new_pos_frame = Frames(self.playhead_frame_shared.load(Ordering::Relaxed));
        if self.playhead_frame != new_pos_frame {
            self.playhead_frame = new_pos_frame;

            let tempo_map = self.tempo_map_shared.get();

            self.playhead_musical = tempo_map.0.frame_to_musical(new_pos_frame);
        }

        self.playhead_musical
    }

    pub(crate) fn tempo_map_shared(&self) -> Shared<SharedCell<(Shared<TempoMap>, u64)>> {
        Shared::clone(&self.tempo_map_shared)
    }
}

#[derive(Debug, Clone, Copy)]
struct Parameters {
    seek_to: (MusicalTime, u64),
    is_playing: bool,
    loop_state: (LoopState, u64),
}

pub struct TransportTask {
    parameters: Shared<SharedCell<Parameters>>,

    tempo_map_shared: Shared<SharedCell<(Shared<TempoMap>, u64)>>,
    tempo_map: Shared<TempoMap>,

    playhead_frame: Frames,
    is_playing: bool,

    loop_state: LoopStateProcInfo,

    loop_back_info: Option<LoopBackInfo>,
    seek_info: Option<SeekInfo>,

    range_checker: RangeChecker,
    next_playhead_frame: Frames,

    seek_to_version: u64,
    loop_state_version: u64,
    tempo_map_version: u64,

    loop_start_beats: EventBeatTime,
    loop_end_beats: EventBeatTime,
    loop_start_seconds: EventSecTime,
    loop_end_seconds: EventSecTime,

    tempo: f64,
    tempo_inc: f64,
    tsig_num: u16,
    tsig_denom: u16,
    bar_start: EventBeatTime,
    bar_number: i32,

    playhead_frame_shared: Arc<AtomicU64>,
}

impl TransportTask {
    pub fn new(
        save_state: Option<TransportSaveState>,
        sample_rate: SampleRate,
        coll_handle: basedrop::Handle,
    ) -> (Self, TransportHandle) {
        let save_state = save_state.unwrap_or_default();

        let parameters = Shared::new(
            &coll_handle,
            SharedCell::new(Shared::new(
                &coll_handle,
                Parameters {
                    seek_to: (save_state.seek_to, 0),
                    is_playing: false,
                    loop_state: (save_state.loop_state, 0),
                },
            )),
        );

        let tempo_map = TempoMap::new(120.0, 4, 4, sample_rate);

        let playhead_frame = tempo_map.musical_to_nearest_frame_round(save_state.seek_to);
        let playhead_frame_shared = Arc::new(AtomicU64::new(playhead_frame.0));
        let loop_state = save_state.loop_state.to_proc(&tempo_map);

        let (loop_start_beats, loop_end_beats, loop_start_seconds, loop_end_seconds) =
            if let LoopState::Active { loop_start, loop_end } = &save_state.loop_state {
                (
                    EventBeatTime::from_f64(loop_start.as_beats_f64()),
                    EventBeatTime::from_f64(loop_end.as_beats_f64()),
                    EventSecTime::from_f64(tempo_map.musical_to_seconds(*loop_start).0),
                    EventSecTime::from_f64(tempo_map.musical_to_seconds(*loop_end).0),
                )
            } else {
                (Default::default(), Default::default(), Default::default(), Default::default())
            };

        let tempo_map = Shared::new(&coll_handle, tempo_map);
        let tempo_map_shared = Shared::new(
            &coll_handle,
            SharedCell::new(Shared::new(&coll_handle, (Shared::clone(&tempo_map), 0))),
        );

        let (tempo, tempo_inc) = tempo_map.bpm_at_frame(playhead_frame);
        let (tsig_num, tsig_denom) = tempo_map.tsig_at_frame(playhead_frame);
        let (bar_number, bar_start) = tempo_map.current_bar_at_frame(playhead_frame);

        (
            TransportTask {
                parameters: Shared::clone(&parameters),
                tempo_map_shared: Shared::clone(&tempo_map_shared),
                tempo_map: Shared::clone(&tempo_map),
                playhead_frame,
                is_playing: false,
                loop_state,
                loop_back_info: None,
                seek_info: None,
                range_checker: RangeChecker::Paused,
                next_playhead_frame: playhead_frame,
                seek_to_version: 0,
                tempo_map_version: 0,
                loop_state_version: 0,
                loop_start_beats,
                loop_end_beats,
                loop_start_seconds,
                loop_end_seconds,
                tempo,
                tempo_inc,
                tsig_num,
                tsig_denom,
                bar_start,
                bar_number,
                playhead_frame_shared: Arc::clone(&playhead_frame_shared),
            },
            TransportHandle {
                parameters,
                tempo_map_shared,
                coll_handle,
                playhead_frame_shared,
                playhead_frame,
                playhead_musical: save_state.seek_to,
                seek_to: save_state.seek_to,
            },
        )
    }

    /// Update the state of this transport.
    pub fn process(&mut self, frames: usize) -> TransportInfo {
        let Parameters { seek_to, is_playing, loop_state } = *self.parameters.get();

        let proc_frames = Frames(frames as u64);

        self.playhead_frame = self.next_playhead_frame;

        let mut loop_state_changed = false;
        if self.loop_state_version != loop_state.1 {
            self.loop_state_version = loop_state.1;
            loop_state_changed = true;
        }

        // Check if the tempo map has changed.
        let mut tempo_map_changed = false;
        let (new_tempo_map, new_version) = &*self.tempo_map_shared.get();
        if self.tempo_map_version != *new_version {
            self.tempo_map_version = *new_version;

            // Get musical time of the playhead using the old tempo map.
            let playhead_musical = self.tempo_map.frame_to_musical(self.playhead_frame);

            self.tempo_map = Shared::clone(new_tempo_map);
            tempo_map_changed = true;

            // Update proc info.
            self.next_playhead_frame =
                self.tempo_map.musical_to_nearest_frame_round(playhead_musical);
            loop_state_changed = true;
        }

        // Seek if gotten a new version of the seek_to value.
        self.seek_info = None;
        if self.seek_to_version != seek_to.1 {
            self.seek_to_version = seek_to.1;

            self.seek_info = Some(SeekInfo { seeked_from_playhead: self.playhead_frame });

            self.next_playhead_frame = self.tempo_map.musical_to_nearest_frame_round(seek_to.0);
        };

        if loop_state_changed {
            let (
                loop_state,
                loop_start_beats,
                loop_end_beats,
                loop_start_seconds,
                loop_end_seconds,
            ) = match &loop_state.0 {
                LoopState::Inactive => (
                    LoopStateProcInfo::Inactive,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
                LoopState::Active { loop_start, loop_end } => (
                    LoopStateProcInfo::Active {
                        loop_start_frame: self
                            .tempo_map
                            .musical_to_nearest_frame_round(*loop_start),
                        loop_end_frame: self.tempo_map.musical_to_nearest_frame_round(*loop_end),
                    },
                    EventBeatTime::from_f64(loop_start.as_beats_f64()),
                    EventBeatTime::from_f64(loop_end.as_beats_f64()),
                    EventSecTime::from_f64(self.tempo_map.musical_to_seconds(*loop_start).0),
                    EventSecTime::from_f64(self.tempo_map.musical_to_seconds(*loop_end).0),
                ),
            };

            self.loop_state = loop_state;
            self.loop_start_beats = loop_start_beats;
            self.loop_end_beats = loop_end_beats;
            self.loop_start_seconds = loop_start_seconds;
            self.loop_end_seconds = loop_end_seconds;
        }

        // We don't need to return a new transport event if nothing has changed and
        // we are not currently playing.
        let do_return_event =
            self.is_playing || is_playing || tempo_map_changed || loop_state_changed;

        self.is_playing = is_playing;
        self.loop_back_info = None;
        self.playhead_frame = self.next_playhead_frame;
        if self.is_playing {
            // Advance the playhead.
            let mut did_loop = false;
            if let LoopStateProcInfo::Active { loop_start_frame, loop_end_frame } = self.loop_state
            {
                if self.playhead_frame < loop_end_frame
                    && self.playhead_frame + proc_frames >= loop_end_frame
                {
                    let first_frames = loop_end_frame - self.playhead_frame;
                    let second_frames = proc_frames - first_frames;

                    self.range_checker = RangeChecker::Looping {
                        end_frame_1: loop_end_frame,
                        start_frame_2: loop_start_frame,
                        end_frame_2: loop_start_frame + second_frames,
                    };

                    self.next_playhead_frame = loop_start_frame + second_frames;

                    self.loop_back_info = Some(LoopBackInfo {
                        loop_start: loop_start_frame,
                        loop_end: loop_end_frame,
                        playhead_end: self.next_playhead_frame,
                    });

                    did_loop = true;
                }
            }

            if !did_loop {
                self.next_playhead_frame = self.playhead_frame + proc_frames;

                self.range_checker = RangeChecker::Playing { end_frame: self.next_playhead_frame };
            }

            let (tempo, tempo_inc) = self.tempo_map.bpm_at_frame(self.playhead_frame);
            let (tsig_num, tsig_denom) = self.tempo_map.tsig_at_frame(self.playhead_frame);
            let (bar_number, bar_start) = self.tempo_map.current_bar_at_frame(self.playhead_frame);

            self.tempo = tempo;
            self.tempo_inc = tempo_inc;
            self.tsig_num = tsig_num;
            self.tsig_denom = tsig_denom;
            self.bar_start = bar_start;
            self.bar_number = bar_number;
        } else {
            self.range_checker = RangeChecker::Paused;
        }

        self.playhead_frame_shared.store(self.next_playhead_frame.0, Ordering::Relaxed);

        let event: Option<ProcEvent> = if do_return_event {
            let song_pos_beats = EventBeatTime::from_f64(
                self.tempo_map.frame_to_musical(self.playhead_frame).as_beats_f64(),
            );
            let song_pos_seconds =
                EventSecTime::from_f64(self.tempo_map.frame_to_seconds(self.playhead_frame).0);

            let mut transport_flags = TransportFlags::HAS_TEMPO
                | TransportFlags::HAS_BEATS_TIMELINE
                | TransportFlags::HAS_SECONDS_TIMELINE
                | TransportFlags::HAS_TIME_SIGNATURE;

            let tempo_inc = if self.is_playing {
                transport_flags |= TransportFlags::IS_PLAYING;
                self.tempo_inc
            } else {
                0.0
            };
            if let LoopStateProcInfo::Active { .. } = self.loop_state {
                transport_flags |= TransportFlags::IS_LOOP_ACTIVE
            }

            Some(
                EventTransport::new(
                    0, // frame the event occurs from the start of the current process cycle
                    0, // space_id
                    EventFlags::empty(),
                    transport_flags,
                    song_pos_beats,
                    song_pos_seconds,
                    self.tempo,
                    tempo_inc,
                    self.loop_start_beats,
                    self.loop_end_beats,
                    self.loop_start_seconds,
                    self.loop_end_seconds,
                    self.bar_start,
                    self.bar_number,
                    self.tsig_num,
                    self.tsig_denom,
                )
                .into(),
            )
        } else {
            None
        };

        TransportInfo {
            playhead_frame: self.playhead_frame,
            is_playing: self.is_playing,
            loop_state: self.loop_state,
            loop_back_info: self.loop_back_info,
            seek_info: self.seek_info,
            range_checker: self.range_checker,
            event,
        }
    }
}

#[derive(Clone)]
pub struct TransportInfo {
    playhead_frame: Frames,
    is_playing: bool,
    loop_state: LoopStateProcInfo,
    loop_back_info: Option<LoopBackInfo>,
    seek_info: Option<SeekInfo>,
    range_checker: RangeChecker,
    event: Option<ProcEvent>,
}

impl std::fmt::Debug for TransportInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("TransportInfo");

        f.field("playhead_frame", &self.playhead_frame);
        f.field("is_playing", &self.is_playing);
        f.field("loop_state", &self.loop_state);
        f.field("loop_back_info", &self.loop_back_info);
        f.field("seek_info", &self.seek_info);
        f.field("range_checker", &self.range_checker);

        f.finish()
    }
}

impl TransportInfo {
    /// When `plackback_state()` is of type `Playing`, then this position is the frame at the start
    /// of this process block. (And `playhead + proc_info.frames` is the end position (exclusive) of
    /// this process block.)
    pub fn playhead_frame(&self) -> Frames {
        self.playhead_frame
    }

    /// Whether or not the timeline is playing.
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// The state of looping on the timeline transport.
    pub fn loop_state(&self) -> LoopStateProcInfo {
        self.loop_state
    }

    /// Returns `Some` if the transport is looping back on this current process cycle.
    pub fn do_loop_back(&self) -> Option<&LoopBackInfo> {
        self.loop_back_info.as_ref()
    }

    /// Returns `Some` if the transport has seeked to a new position this current process cycle.
    pub fn did_seek(&self) -> Option<&SeekInfo> {
        self.seek_info.as_ref()
    }

    /// Use this to check whether a range of frames lies inside this current process block.
    ///
    /// This will properly handle playing, paused, and looping conditions.
    ///
    /// This will always return false when the transport status is `Paused` or `Clear`.
    ///
    /// * `start` - The start of the range (inclusive).
    /// * `end` - The end of the range (exclusive).
    pub fn is_range_active(&self, start: Frames, end: Frames) -> bool {
        self.range_checker.is_range_active(self.playhead_frame, start, end)
    }

    /// Use this to check whether a particular frame lies inside this current process block.
    ///
    /// This will properly handle playing, paused, and looping conditions.
    ///
    /// This will always return false when the transport status is `Paused` or `Clear`.
    pub fn is_frame_active(&self, frame: Frames) -> bool {
        self.range_checker.is_frame_active(self.playhead_frame, frame)
    }

    pub fn event(&self) -> Option<&ProcEvent> {
        self.event.as_ref()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoopBackInfo {
    /// The frame where the loop starts on the timeline (inclusive).
    pub loop_start: Frames,

    /// The frame where the loop ends on the timeline (exclusive).
    pub loop_end: Frames,

    /// The frame where the playhead will end on this current process cycle (exclusive).
    pub playhead_end: Frames,
}

#[derive(Debug, Clone, Copy)]
pub struct SeekInfo {
    /// This is what the playhead would have been if the transport did not seek this
    /// process cycle.
    pub seeked_from_playhead: Frames,
}

#[derive(Debug, Clone, Copy)]
enum RangeChecker {
    Playing {
        /// The end frame (exclusive)
        end_frame: Frames,
    },
    Looping {
        /// The end frame of the first part before the loop-back (exclusive)
        end_frame_1: Frames,
        /// The start frame of the second part after the loop-back (inclusive)
        start_frame_2: Frames,
        /// The end frame of the second part after the loop-back (exclusive)
        end_frame_2: Frames,
    },
    Paused,
}

impl RangeChecker {
    #[inline]
    pub fn is_range_active(&self, playhead: Frames, start: Frames, end: Frames) -> bool {
        match self {
            RangeChecker::Playing { end_frame } => playhead < end && start < *end_frame,
            RangeChecker::Looping { end_frame_1, start_frame_2, end_frame_2 } => {
                (playhead < end && start < *end_frame_1)
                    || (*start_frame_2 < end && start < *end_frame_2)
            }
            RangeChecker::Paused => false,
        }
    }
    #[inline]
    pub fn is_frame_active(&self, playhead: Frames, frame: Frames) -> bool {
        match self {
            RangeChecker::Playing { end_frame } => frame >= playhead && frame < *end_frame,
            RangeChecker::Looping { end_frame_1, start_frame_2, end_frame_2 } => {
                (frame >= playhead && frame < *end_frame_1)
                    || (frame >= *start_frame_2 && frame < *end_frame_2)
            }
            RangeChecker::Paused => false,
        }
    }
}

/// The status of looping on this transport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopState {
    /// The transport is not currently looping.
    Inactive,
    /// The transport is currently looping.
    Active {
        /// The start of the loop (inclusive).
        loop_start: MusicalTime,
        /// The end of the loop (exclusive).
        loop_end: MusicalTime,
    },
}

impl LoopState {
    fn to_proc(&self, tempo_map: &TempoMap) -> LoopStateProcInfo {
        match self {
            LoopState::Inactive => LoopStateProcInfo::Inactive,
            &LoopState::Active { loop_start, loop_end } => LoopStateProcInfo::Active {
                loop_start_frame: tempo_map.musical_to_nearest_frame_round(loop_start),
                loop_end_frame: tempo_map.musical_to_nearest_frame_round(loop_end),
            },
        }
    }
}

/// The status of looping on this transport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopStateProcInfo {
    /// The transport is not currently looping.
    Inactive,
    /// The transport is currently looping.
    Active {
        /// The start of the loop (inclusive).
        loop_start_frame: Frames,
        /// The end of the loop (exclusive).
        loop_end_frame: Frames,
    },
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_range_checker() {
        use super::Frames;
        use super::RangeChecker;

        let playhead = Frames(3);
        let r = RangeChecker::Playing { end_frame: Frames(10) };

        assert!(r.is_range_active(playhead, Frames(5), Frames(12)));
        assert!(r.is_range_active(playhead, Frames(0), Frames(5)));
        assert!(r.is_range_active(playhead, Frames(3), Frames(10)));
        assert!(!r.is_range_active(playhead, Frames(10), Frames(12)));
        assert!(!r.is_range_active(playhead, Frames(12), Frames(14)));
        assert!(r.is_range_active(playhead, Frames(9), Frames(12)));
        assert!(!r.is_range_active(playhead, Frames(0), Frames(2)));
        assert!(!r.is_range_active(playhead, Frames(0), Frames(3)));
        assert!(r.is_range_active(playhead, Frames(0), Frames(4)));
        assert!(r.is_range_active(playhead, Frames(4), Frames(8)));

        assert!(!r.is_frame_active(playhead, Frames(0)));
        assert!(!r.is_frame_active(playhead, Frames(2)));
        assert!(r.is_frame_active(playhead, Frames(3)));
        assert!(r.is_frame_active(playhead, Frames(9)));
        assert!(!r.is_frame_active(playhead, Frames(10)));
        assert!(!r.is_frame_active(playhead, Frames(11)));

        let playhead = Frames(20);
        let r = RangeChecker::Looping {
            end_frame_1: Frames(24),
            start_frame_2: Frames(2),
            end_frame_2: Frames(10),
        };

        assert!(r.is_range_active(playhead, Frames(0), Frames(5)));
        assert!(r.is_range_active(playhead, Frames(0), Frames(3)));
        assert!(!r.is_range_active(playhead, Frames(0), Frames(2)));
        assert!(r.is_range_active(playhead, Frames(15), Frames(27)));
        assert!(r.is_range_active(playhead, Frames(15), Frames(21)));
        assert!(!r.is_range_active(playhead, Frames(15), Frames(20)));
        assert!(r.is_range_active(playhead, Frames(4), Frames(23)));
        assert!(r.is_range_active(playhead, Frames(0), Frames(30)));
        assert!(!r.is_range_active(playhead, Frames(10), Frames(18)));
        assert!(!r.is_range_active(playhead, Frames(12), Frames(20)));

        assert!(!r.is_frame_active(playhead, Frames(0)));
        assert!(r.is_frame_active(playhead, Frames(2)));
        assert!(r.is_frame_active(playhead, Frames(3)));
        assert!(!r.is_frame_active(playhead, Frames(10)));
        assert!(!r.is_frame_active(playhead, Frames(15)));
        assert!(r.is_frame_active(playhead, Frames(20)));
        assert!(r.is_frame_active(playhead, Frames(23)));
        assert!(!r.is_frame_active(playhead, Frames(24)));
        assert!(!r.is_frame_active(playhead, Frames(25)));
    }
}
