use clack_host::utils::BeatTime;
use meadowlark_core_types::time::{FrameTime, MusicalTime, SampleRate, SecondsF64};

// TODO: Make tempo map work like series of automation lines/curves between points in time.

/// A map of all tempo changes in the current project.
///
/// Here is the intended workflow for keeping time:
/// 1. The GUI/non-realtime thread stores all events in `MusicalTime` (unit of beats).
/// 2. The GUI/non-realtime thread creates a new `TempoMap` on project startup and whenever anything about
/// the tempo changes. It this sends this new `TempoMap` to the realtime-thread.
/// 3. When the realtime thread detects a new `TempoMap`, all processors with events stored in `MusicalTime` use
/// the `TempoMap` plus the `SampleRate` to convert each `MusicalTime` into the corresponding discrete time in frames (i64) (or sub-frames).
/// It keeps this new i64 for all future use (until a new `TempoMap` is recieved).
/// 4. When playback occurs, the realtime-thread keeps tracks of the number of discrete frames that have elapsed. It
/// sends this "playhead" to each of the processors, which in turn compares it to it's own previously calculated i64 to know when
/// events should be played.
/// 5. Once the realtime thread is done processing a buffer, it uses the `TempoMap` plus this `SampleRate` to convert this
/// playhead into the corresponding `MusicalTime`. It then sends this to the GUI/non-realtime thread for visual
/// feedback of the playhead.
/// 6. When the GUI/non-realtime thread wants to manually change the position of the playhead, it sends the `MusicalTime` that
/// should be seeked to the realtime thread. The realtime thread then uses it, the `TempoMap`, and the `SampleRate` to find
/// the nearest (floored) frame to set as the new playhead.
#[derive(Debug, Clone)]
pub struct TempoMap {
    pub sample_rate: SampleRate,

    /// Temporary static tempo
    beats_per_second: f64,
    seconds_per_beat: f64,

    /// Temporary static time signature
    tsig_num: u16,
    tsig_denom: u16,
}

impl TempoMap {
    /// Temporary static tempo and time signature
    pub fn new(bpm: f64, tsig_num: u16, tsig_denom: u16, sample_rate: SampleRate) -> Self {
        assert_ne!(tsig_num, 0);
        assert_ne!(tsig_denom, 0);

        TempoMap {
            beats_per_second: bpm / 60.0,
            seconds_per_beat: 60.0 / bpm,
            tsig_num,
            tsig_denom,
            sample_rate,
        }
    }

    pub fn bpm_at_musical_time(&self, _musical_time: MusicalTime) -> f64 {
        // temporary static tempo
        self.beats_per_second * 60.0
    }

    /// `(tempo in bpm, tempo increment for each sample until the next time info event)
    pub fn bpm_at_frame(&self, _frame: FrameTime) -> (f64, f64) {
        // temporary static tempo
        (self.beats_per_second * 60.0, 0.0)
    }

    /// `(numerator, denomitator)`
    pub fn tsig_at_musical_time(&self, _musical_time: MusicalTime) -> (u16, u16) {
        // temporary static time signature
        (self.tsig_num, self.tsig_denom)
    }

    /// `(numerator, denomitator)`
    pub fn tsig_at_frame(&self, _frame: FrameTime) -> (u16, u16) {
        // temporary static time signature
        (self.tsig_num, self.tsig_denom)
    }

    /// `(the bar number of the song, the beat where the bar starts)`
    pub fn current_bar_at_frame(&self, frame: FrameTime) -> (i32, BeatTime) {
        // temporary static tempo and time signature
        let current_beat = self.frame_to_musical(frame).beats() as i64;

        let bar_number = current_beat / i64::from(self.tsig_num);
        let bar_start_beat = bar_number * i64::from(self.tsig_denom);

        (bar_number as i32, BeatTime::from_int(bar_start_beat))
    }

    /// Temporary static tempo
    pub fn set_bpm(&mut self, bpm: f64) {
        self.beats_per_second = bpm / 60.0;
        self.seconds_per_beat = 60.0 / bpm;
    }

    /// Temporary static time signature
    pub fn set_time_signature(&mut self, tsig_num: u16, tsig_denom: u16) {
        assert_ne!(tsig_num, 0);
        assert_ne!(tsig_denom, 0);

        self.tsig_num = tsig_num;
        self.tsig_denom = tsig_denom;
    }

    /// Convert the given `MusicalTime` into the corresponding time in `SecondsF64`.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn musical_to_seconds(&self, musical_time: MusicalTime) -> SecondsF64 {
        // temporary static tempo
        SecondsF64(musical_time.as_beats_f64() * self.seconds_per_beat)
    }

    /// Convert the given `SecondsF64` into the corresponding `MusicalTime`.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn seconds_to_musical(&self, seconds: SecondsF64) -> MusicalTime {
        // temporary static tempo
        MusicalTime::from_beats_f64(seconds.0 * self.beats_per_second)
    }

    /// Convert the given `FrameTime` time into the corresponding `MusicalTime`.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn frame_to_musical(&self, frame: FrameTime) -> MusicalTime {
        // temporary static tempo
        MusicalTime::from_beats_f64(
            frame.to_seconds_f64(self.sample_rate).0 * self.beats_per_second,
        )
    }

    /// Convert the given `FrameTime` time into the corresponding time in `SecondsF64`.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn frame_to_seconds(&self, frame: FrameTime) -> SecondsF64 {
        // temporary static tempo
        SecondsF64::from_frame(frame, self.sample_rate)
    }

    /// Convert the given `MusicalTime` into the corresponding discrete `FrameTime` time.
    /// This will be rounded to the nearest frame.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn musical_to_nearest_frame_round(&self, musical_time: MusicalTime) -> FrameTime {
        // temporary static tempo
        self.musical_to_seconds(musical_time).to_nearest_frame_round(self.sample_rate)
    }

    /// Convert the given `SecondsF64` into the corresponding discrete `FrameTime` time.
    /// This will be rounded to the nearest frame.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn seconds_to_nearest_frame_round(&self, seconds: SecondsF64) -> FrameTime {
        // temporary static tempo
        seconds.to_nearest_frame_round(self.sample_rate)
    }

    /// Convert the given `MusicalTime` into the corresponding discrete `FrameTime` time.
    /// This will be floored to the nearest frame.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn musical_to_nearest_frame_floor(&self, musical_time: MusicalTime) -> FrameTime {
        // temporary static tempo
        self.musical_to_seconds(musical_time).to_nearest_frame_floor(self.sample_rate)
    }

    /// Convert the given `SecondsF64` into the corresponding discrete `FrameTime` time.
    /// This will be floored to the nearest frame.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn seconds_to_nearest_frame_floor(&self, seconds: SecondsF64) -> FrameTime {
        // temporary static tempo
        seconds.to_nearest_frame_floor(self.sample_rate)
    }

    /// Convert the given `MusicalTime` into the corresponding discrete `FrameTime` time.
    /// This will be ceil-ed to the nearest frame.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn musical_to_nearest_frame_ceil(&self, musical_time: MusicalTime) -> FrameTime {
        // temporary static tempo
        self.musical_to_seconds(musical_time).to_nearest_frame_ceil(self.sample_rate)
    }

    /// Convert the given `SecondsF64` into the corresponding discrete `FrameTime` time.
    /// This will be ceil-ed to the nearest frame.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn seconds_to_nearest_frame_ceil(&self, seconds: SecondsF64) -> FrameTime {
        // temporary static tempo
        seconds.to_nearest_frame_ceil(self.sample_rate)
    }

    /// Convert the given `MusicalTime` into the corresponding discrete `FrameTime` time
    /// floored to the nearest frame, while also returning the fractional sub-frame part.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn musical_to_sub_frame(&self, musical_time: MusicalTime) -> (FrameTime, f64) {
        // temporary static tempo
        self.musical_to_seconds(musical_time).to_sub_frame(self.sample_rate)
    }

    /// Convert the given `SecondsF64` into the corresponding discrete `FrameTime` time
    /// floored to the nearest frame, while also returning the fractional sub-frame part.
    ///
    /// Note that this must be re-calculated after recieving a new `TempoMap`.
    #[inline]
    pub fn seconds_to_sub_frame(&self, seconds: SecondsF64) -> (FrameTime, f64) {
        // temporary static tempo
        seconds.to_sub_frame(self.sample_rate)
    }
}

impl Default for TempoMap {
    fn default() -> Self {
        TempoMap::new(110.0, 4, 4, SampleRate::default())
    }
}
