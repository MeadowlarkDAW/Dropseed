use clack_host::events::event_types::TransportEvent;
use meadowlark_core_types::time::{FrameTime, MusicalTime};

mod declick;
mod tempo_map;
pub use tempo_map::TempoMap;

pub use declick::{DeclickBuffers, DeclickInfo, DEFAULT_DECLICK_TIME};

#[derive(Clone)]
pub struct TransportInfo {
    playhead_frame: FrameTime,
    is_playing: bool,
    loop_state: LoopStateProcInfo,
    loop_back_info: Option<LoopBackInfo>,
    seek_info: Option<SeekInfo>,
    range_checker: RangeChecker,
    event: Option<TransportEvent>,
    declick: Option<DeclickInfo>,
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
    pub fn _new(
        playhead_frame: FrameTime,
        is_playing: bool,
        loop_state: LoopStateProcInfo,
        loop_back_info: Option<LoopBackInfo>,
        seek_info: Option<SeekInfo>,
        range_checker: RangeChecker,
        event: Option<TransportEvent>,
        declick: Option<DeclickInfo>,
    ) -> Self {
        Self {
            playhead_frame,
            is_playing,
            loop_state,
            loop_back_info,
            seek_info,
            range_checker,
            event,
            declick,
        }
    }

    /// When `plackback_state()` is of type `Playing`, then this position is the frame at the start
    /// of this process block. (And `playhead + proc_info.FrameTime` is the end position (exclusive) of
    /// this process block.)
    pub fn playhead_frame(&self) -> FrameTime {
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

    /// Use this to check whether a range of FrameTime lies inside this current process block.
    ///
    /// This will properly handle playing, paused, and looping conditions.
    ///
    /// This will always return false when the transport status is `Paused` or `Clear`.
    ///
    /// * `start` - The start of the range (inclusive).
    /// * `end` - The end of the range (exclusive).
    pub fn is_range_active(&self, start: FrameTime, end: FrameTime) -> bool {
        self.range_checker.is_range_active(self.playhead_frame, start, end)
    }

    /// Use this to check whether a particular frame lies inside this current process block.
    ///
    /// This will properly handle playing, paused, and looping conditions.
    ///
    /// This will always return false when the transport status is `Paused` or `Clear`.
    pub fn is_frame_active(&self, frame: FrameTime) -> bool {
        self.range_checker.is_frame_active(self.playhead_frame, frame)
    }

    pub fn event(&self) -> Option<&TransportEvent> {
        self.event.as_ref()
    }

    pub fn declick_info(&self) -> Option<&DeclickInfo> {
        self.declick.as_ref()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LoopBackInfo {
    /// The frame where the loop starts on the timeline (inclusive).
    pub loop_start: FrameTime,

    /// The frame where the loop ends on the timeline (exclusive).
    pub loop_end: FrameTime,

    /// The frame where the playhead will end on this current process cycle (exclusive).
    pub playhead_end: FrameTime,
}

#[derive(Debug, Clone, Copy)]
pub struct SeekInfo {
    /// This is what the playhead would have been if the transport did not seek this
    /// process cycle.
    pub seeked_from_playhead: FrameTime,
}

#[derive(Debug, Clone, Copy)]
pub enum RangeChecker {
    Playing {
        /// The end frame (exclusive)
        end_frame: FrameTime,
    },
    Looping {
        /// The end frame of the first part before the loop-back (exclusive)
        end_frame_1: FrameTime,
        /// The start frame of the second part after the loop-back (inclusive)
        start_frame_2: FrameTime,
        /// The end frame of the second part after the loop-back (exclusive)
        end_frame_2: FrameTime,
    },
    Paused,
}

impl RangeChecker {
    #[inline]
    pub fn is_range_active(&self, playhead: FrameTime, start: FrameTime, end: FrameTime) -> bool {
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
    pub fn is_frame_active(&self, playhead: FrameTime, frame: FrameTime) -> bool {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn as_proc_info(&self, tempo_map: &TempoMap) -> LoopStateProcInfo {
        match self {
            LoopState::Inactive => LoopStateProcInfo::Inactive,
            LoopState::Active { loop_start, loop_end } => LoopStateProcInfo::Active {
                loop_start_frame: tempo_map.musical_to_nearest_frame_round(*loop_start),
                loop_end_frame: tempo_map.musical_to_nearest_frame_round(*loop_end),
            },
        }
    }
}

/// The status of looping on this transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopStateProcInfo {
    /// The transport is not currently looping.
    Inactive,
    /// The transport is currently looping.
    Active {
        /// The start of the loop (inclusive).
        loop_start_frame: FrameTime,
        /// The end of the loop (exclusive).
        loop_end_frame: FrameTime,
    },
}

#[cfg(test)]
mod tests {
    #[test]
    fn transport_range_checker() {
        use super::FrameTime;
        use super::RangeChecker;

        let playhead = FrameTime(3);
        let r = RangeChecker::Playing { end_frame: FrameTime(10) };

        assert!(r.is_range_active(playhead, FrameTime(5), FrameTime(12)));
        assert!(r.is_range_active(playhead, FrameTime(0), FrameTime(5)));
        assert!(r.is_range_active(playhead, FrameTime(3), FrameTime(10)));
        assert!(!r.is_range_active(playhead, FrameTime(10), FrameTime(12)));
        assert!(!r.is_range_active(playhead, FrameTime(12), FrameTime(14)));
        assert!(r.is_range_active(playhead, FrameTime(9), FrameTime(12)));
        assert!(!r.is_range_active(playhead, FrameTime(0), FrameTime(2)));
        assert!(!r.is_range_active(playhead, FrameTime(0), FrameTime(3)));
        assert!(r.is_range_active(playhead, FrameTime(0), FrameTime(4)));
        assert!(r.is_range_active(playhead, FrameTime(4), FrameTime(8)));

        assert!(!r.is_frame_active(playhead, FrameTime(0)));
        assert!(!r.is_frame_active(playhead, FrameTime(2)));
        assert!(r.is_frame_active(playhead, FrameTime(3)));
        assert!(r.is_frame_active(playhead, FrameTime(9)));
        assert!(!r.is_frame_active(playhead, FrameTime(10)));
        assert!(!r.is_frame_active(playhead, FrameTime(11)));

        let playhead = FrameTime(20);
        let r = RangeChecker::Looping {
            end_frame_1: FrameTime(24),
            start_frame_2: FrameTime(2),
            end_frame_2: FrameTime(10),
        };

        assert!(r.is_range_active(playhead, FrameTime(0), FrameTime(5)));
        assert!(r.is_range_active(playhead, FrameTime(0), FrameTime(3)));
        assert!(!r.is_range_active(playhead, FrameTime(0), FrameTime(2)));
        assert!(r.is_range_active(playhead, FrameTime(15), FrameTime(27)));
        assert!(r.is_range_active(playhead, FrameTime(15), FrameTime(21)));
        assert!(!r.is_range_active(playhead, FrameTime(15), FrameTime(20)));
        assert!(r.is_range_active(playhead, FrameTime(4), FrameTime(23)));
        assert!(r.is_range_active(playhead, FrameTime(0), FrameTime(30)));
        assert!(!r.is_range_active(playhead, FrameTime(10), FrameTime(18)));
        assert!(!r.is_range_active(playhead, FrameTime(12), FrameTime(20)));

        assert!(!r.is_frame_active(playhead, FrameTime(0)));
        assert!(r.is_frame_active(playhead, FrameTime(2)));
        assert!(r.is_frame_active(playhead, FrameTime(3)));
        assert!(!r.is_frame_active(playhead, FrameTime(10)));
        assert!(!r.is_frame_active(playhead, FrameTime(15)));
        assert!(r.is_frame_active(playhead, FrameTime(20)));
        assert!(r.is_frame_active(playhead, FrameTime(23)));
        assert!(!r.is_frame_active(playhead, FrameTime(24)));
        assert!(!r.is_frame_active(playhead, FrameTime(25)));
    }
}
