use super::audio_port_buffer::{AudioPortBuffer, AudioPortBufferMut};

/// The status of a call to a node's`process()` method.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Processing failed. The output buffer must be discarded.
    Error = 0,

    /// Processing succeeded, keep processing.
    Continue = 1,

    /// Processing succeeded, keep processing if the output is not quiet.
    ContinueIfNotQuiet = 2,

    /// Rely upon the plugin's tail to determine if the plugin should continue to process.
    Tail = 3,

    /// Processing succeeded, but no more processing is required until
    /// the next event or variation in audio input.
    Sleep = 4,
}

#[cfg(any(feature = "c-bindings", feature = "clap-hosting"))]
impl ProcessStatus {
    pub(crate) fn from_raw(s: i32) -> Self {
        if s < 0 || s > 4 {
            Self::Error
        } else {
            // Safe because we checked that the value is within bounds, and
            // our enum is represented as an `i32` value.
            unsafe { *(&s as *const i32 as *const Self) }
        }
    }

    pub(crate) fn as_raw(&self) -> i32 {
        *self as i32
    }
}

pub struct ProcData<'a> {
    /// A steady sample time counter.
    ///
    /// This field can be used to calculate the sleep duration between two process calls.
    /// This value may be specific to this plugin instance and have no relation to what
    /// other plugin instances may receive.
    ///
    /// This will be -1 if not available, otherwise the value will be greater or equal to
    /// 0, and will be increased by at least `frames_count` for the next call to process.
    pub steady_time: i64,

    /// Number of frames to process
    pub frame: usize,

    // TODO: transport
    pub audio_in: &'a [AudioPortBuffer<'a>],
    pub audio_out: &'a [AudioPortBufferMut<'a>],
    // TODO: events
}
