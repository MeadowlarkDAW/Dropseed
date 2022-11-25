use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use basedrop::{Shared, SharedCell};
use clack_host::events::event_types::{TransportEvent, TransportEventFlags};
use clack_host::events::{EventFlags, EventHeader};
use clack_host::utils::{BeatTime, SecondsTime};
use dropseed_plugin_api::transport::{
    LoopBackInfo, LoopState, LoopStateProcInfo, RangeChecker, SeekInfo, TempoMap, TransportInfo,
};
use meadowlark_core_types::time::{FrameTime, MusicalTime, SampleRate, SecondsF64};

mod declick;
use declick::{JumpInfo, TransportDeclick};

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
    playhead_frame: FrameTime,
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
        let new_pos_frame = FrameTime(self.playhead_frame_shared.load(Ordering::Relaxed));
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

    playhead_frame: FrameTime,
    is_playing: bool,

    loop_state: LoopStateProcInfo,

    loop_back_info: Option<LoopBackInfo>,
    seek_info: Option<SeekInfo>,

    range_checker: RangeChecker,
    next_playhead_frame: FrameTime,

    seek_to_version: u64,
    loop_state_version: u64,
    tempo_map_version: u64,

    loop_start_beats: BeatTime,
    loop_end_beats: BeatTime,
    loop_start_seconds: SecondsTime,
    loop_end_seconds: SecondsTime,

    tempo: f64,
    tempo_inc: f64,
    tsig_num: u16,
    tsig_denom: u16,
    bar_start: BeatTime,
    bar_number: i32,

    playhead_frame_shared: Arc<AtomicU64>,

    declick: Option<TransportDeclick>,
}

impl TransportTask {
    pub fn new(
        save_state: Option<TransportSaveState>,
        sample_rate: SampleRate,
        max_frames: usize,
        declick_time: Option<SecondsF64>,
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
        let loop_state = save_state.loop_state.as_proc_info(&tempo_map);

        let (loop_start_beats, loop_end_beats, loop_start_seconds, loop_end_seconds) =
            if let LoopState::Active { loop_start, loop_end } = &save_state.loop_state {
                (
                    BeatTime::from_float(loop_start.as_beats_f64()),
                    BeatTime::from_float(loop_end.as_beats_f64()),
                    SecondsTime::from_float(tempo_map.musical_to_seconds(*loop_start).0),
                    SecondsTime::from_float(tempo_map.musical_to_seconds(*loop_end).0),
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

        let declick =
            declick_time.map(|d| TransportDeclick::new(max_frames, d, sample_rate, &coll_handle));

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
                declick,
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

        let proc_frames = FrameTime(frames as u64);

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
                    BeatTime::from_float(loop_start.as_beats_f64()),
                    BeatTime::from_float(loop_end.as_beats_f64()),
                    SecondsTime::from_float(self.tempo_map.musical_to_seconds(*loop_start).0),
                    SecondsTime::from_float(self.tempo_map.musical_to_seconds(*loop_end).0),
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
                }
            }

            if self.loop_back_info.is_none() {
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

        let event: Option<TransportEvent> = if do_return_event {
            let song_pos_beats = BeatTime::from_float(
                self.tempo_map.frame_to_musical(self.playhead_frame).as_beats_f64(),
            );
            let song_pos_seconds =
                SecondsTime::from_float(self.tempo_map.frame_to_seconds(self.playhead_frame).0);

            let mut transport_flags = TransportEventFlags::HAS_TEMPO
                | TransportEventFlags::HAS_BEATS_TIMELINE
                | TransportEventFlags::HAS_SECONDS_TIMELINE
                | TransportEventFlags::HAS_TIME_SIGNATURE;

            let tempo_inc = if self.is_playing {
                transport_flags |= TransportEventFlags::IS_PLAYING;
                self.tempo_inc
            } else {
                0.0
            };
            if let LoopStateProcInfo::Active { .. } = self.loop_state {
                transport_flags |= TransportEventFlags::IS_LOOP_ACTIVE
            }

            Some(TransportEvent {
                header: EventHeader::new_core(0, EventFlags::empty()),

                flags: transport_flags,

                song_pos_beats,
                song_pos_seconds,

                tempo: self.tempo,
                tempo_inc,

                loop_start_beats: self.loop_start_beats,
                loop_end_beats: self.loop_end_beats,
                loop_start_seconds: self.loop_start_seconds,
                loop_end_seconds: self.loop_end_seconds,

                bar_start: self.bar_start,
                bar_number: self.bar_number,

                time_signature_numerator: self.tsig_num as i16,
                time_signature_denominator: self.tsig_denom as i16,
            })
        } else {
            None
        };

        let declick_info = if let Some(declick) = &mut self.declick {
            let jump_info = if let Some(info) = &self.loop_back_info {
                JumpInfo::Looped(info)
            } else if let Some(info) = &self.seek_info {
                JumpInfo::Seeked(info)
            } else {
                JumpInfo::None
            };

            declick.process(self.playhead_frame, frames, self.is_playing, jump_info);

            Some(declick.get_info())
        } else {
            None
        };

        TransportInfo::_new(
            self.playhead_frame,
            self.is_playing,
            self.loop_state,
            self.loop_back_info,
            self.seek_info,
            self.range_checker,
            event,
            declick_info,
        )
    }
}
