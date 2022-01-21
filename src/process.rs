use clap_sys::{
    audio_buffer::clap_audio_buffer,
    process::{
        clap_process, clap_process_status, CLAP_PROCESS_CONTINUE,
        CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_ERROR, CLAP_PROCESS_SLEEP,
    },
};
use rusty_daw_core::Frames;
use smallvec::SmallVec;

use crate::audio_buffer::{ClapAudioBuffer, InternalAudioBuffer};

/// The status of a call to a plugin's `process()` method.
#[non_exhaustive]
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum ProcessStatus {
    /// Processing failed. The output buffer must be discarded.
    Error = CLAP_PROCESS_ERROR,

    /// Processing succeeded, keep processing.
    Continue = CLAP_PROCESS_CONTINUE,

    /// Processing succeeded, keep processing if the output is not quiet.
    ContinueIfNotQuiet = CLAP_PROCESS_CONTINUE_IF_NOT_QUIET,

    /// Processing succeeded, but no more processing is required until
    /// the next event or variation in audio input.
    Sleep = CLAP_PROCESS_SLEEP,
}

impl ProcessStatus {
    pub fn from_clap(status: clap_process_status) -> Option<ProcessStatus> {
        match status {
            CLAP_PROCESS_ERROR => Some(ProcessStatus::Error),
            CLAP_PROCESS_CONTINUE => Some(ProcessStatus::Error),
            CLAP_PROCESS_CONTINUE_IF_NOT_QUIET => Some(ProcessStatus::Error),
            CLAP_PROCESS_SLEEP => Some(ProcessStatus::Error),
            _ => None,
        }
    }

    pub fn to_clap(&self) -> clap_process_status {
        match self {
            ProcessStatus::Error => CLAP_PROCESS_ERROR,
            ProcessStatus::Continue => CLAP_PROCESS_CONTINUE,
            ProcessStatus::ContinueIfNotQuiet => CLAP_PROCESS_CONTINUE_IF_NOT_QUIET,
            ProcessStatus::Sleep => CLAP_PROCESS_SLEEP,
        }
    }
}

pub enum MonoInOutStatus<'a, T: Sized + Copy + Clone + Send + Default + 'static> {
    /// The host has given a single buffer for both the input and output port.
    InPlace(&'a mut [T]),

    /// The host has given a separate input and output buffer.
    Separate { input: &'a [T], output: &'a mut [T] },

    /// The host has not given a main mono output buffer.
    NoMonoOut,
}

pub enum StereoInOutStatus<'a, T: Sized + Copy + Clone + Send + Default + 'static> {
    /// The host has given a single buffer for both the input and output port.
    InPlace((&'a mut [T], &'a mut [T])),

    /// The host has given a separate input and output buffer.
    Separate { input: (&'a [T], &'a [T]), output: (&'a mut [T], &'a mut [T]) },

    /// The host has not given a stereo output buffer.
    NoStereoOut,
}

/// The port buffers (for use with external CLAP plugins).
pub(crate) struct ClapProcAudioPorts {
    audio_inputs: SmallVec<[ClapAudioBuffer; 1]>,
    audio_outputs: SmallVec<[ClapAudioBuffer; 1]>,

    raw_audio_inputs: SmallVec<[*const clap_audio_buffer; 1]>,
    raw_audio_outputs: SmallVec<[*const clap_audio_buffer; 1]>,

    audio_inputs_count: u32,
    audio_outputs_count: u32,
}

impl ClapProcAudioPorts {
    pub(crate) fn debug_fields(&self, f: &mut std::fmt::DebugStruct) {
        if !self.audio_inputs.is_empty() {
            let mut s = format!("[{:?}", &self.audio_inputs[0]);
            for b in self.audio_inputs.iter().skip(1) {
                s.push_str(&format!(" ,{:?}", b));
            }
            s.push_str("]");

            f.field("audio_in", &s);
        }
        if !self.audio_outputs.is_empty() {
            let mut s = format!("[{:?}", &self.audio_outputs[0]);
            for b in self.audio_outputs.iter().skip(1) {
                s.push_str(&format!(" ,{:?}", b));
            }
            s.push_str("]");

            f.field("audio_out", &s);
        }
    }

    pub(crate) fn new(
        audio_inputs: SmallVec<[ClapAudioBuffer; 1]>,
        audio_outputs: SmallVec<[ClapAudioBuffer; 1]>,
    ) -> Self {
        let mut raw_audio_inputs: SmallVec<[*const clap_audio_buffer; 1]> =
            SmallVec::with_capacity(audio_inputs.len());
        let mut raw_audio_outputs: SmallVec<[*const clap_audio_buffer; 1]> =
            SmallVec::with_capacity(audio_outputs.len());
        for _ in 0..audio_inputs.len() {
            raw_audio_inputs.push(std::ptr::null());
        }
        for _ in 0..audio_outputs.len() {
            raw_audio_outputs.push(std::ptr::null());
        }

        let audio_inputs_count = audio_inputs.len() as u32;
        let audio_outputs_count = audio_outputs.len() as u32;

        Self {
            audio_inputs,
            audio_outputs,
            raw_audio_inputs,
            raw_audio_outputs,
            audio_inputs_count,
            audio_outputs_count,
        }
    }

    pub(crate) fn prepare(&mut self, proc: &mut clap_process) {
        // TODO: We could probably use `Pin` or something to avoid collecting
        // the array of pointers every time.

        // Safe because we own all this data, and we made sure that the SmallVecs
        // have the correct size in the constructor.
        unsafe {
            for i in 0..self.audio_inputs.len() {
                *self.raw_audio_inputs.get_unchecked_mut(i) = self.audio_inputs[i].as_raw();
            }
            for i in 0..self.audio_outputs.len() {
                *self.raw_audio_outputs.get_unchecked_mut(i) = self.audio_outputs[i].as_raw();
            }
        }

        proc.audio_inputs = if !self.raw_audio_inputs.is_empty() {
            self.raw_audio_inputs[0]
        } else {
            std::ptr::null()
        };
        proc.audio_outputs = if !self.raw_audio_outputs.is_empty() {
            self.raw_audio_outputs[0]
        } else {
            std::ptr::null()
        };

        proc.audio_inputs_count = self.audio_inputs_count;
        proc.audio_outputs_count = self.audio_outputs_count;
    }
}

/// The audio port buffers (for use with internal plugins).
pub struct ProcAudioPorts<T: Sized + Copy + Clone + Send + Default + 'static> {
    /// The main audio input buffer.
    ///
    /// Note this may be `None` even when a main input port exists.
    /// In that case it means the host has given the same buffer for
    /// the main input and output ports (process replacing).
    pub main_in: Option<InternalAudioBuffer<T>>,

    /// The main audio output buffer.
    pub main_out: Option<InternalAudioBuffer<T>>,

    /// The extra inputs buffers (not including the main input buffer).
    pub extra_inputs: Vec<InternalAudioBuffer<T>>,

    /// The extra output buffers (not including the main input buffer).
    pub extra_outputs: Vec<InternalAudioBuffer<T>>,
}

impl<T: Sized + Copy + Clone + Send + Default + 'static> ProcAudioPorts<T> {
    pub(crate) fn debug_fields(&self, f: &mut std::fmt::DebugStruct) {
        if let Some(b) = &self.main_in {
            f.field("main_in", b);
        }
        if let Some(b) = &self.main_out {
            f.field("main_out", b);
        }
        if !self.extra_inputs.is_empty() {
            let mut s = format!("[{:?}", &self.extra_inputs[0]);
            for b in self.extra_inputs.iter().skip(1) {
                s.push_str(&format!(" ,{:?}", b));
            }
            s.push_str("]");

            f.field("extra_in", &s);
        }
        if !self.extra_outputs.is_empty() {
            let mut s = format!("[{:?}", &self.extra_outputs[0]);
            for b in self.extra_outputs.iter().skip(1) {
                s.push_str(&format!(" ,{:?}", b));
            }
            s.push_str("]");

            f.field("extra_out", &s);
        }
    }

    /// A helper method to retrieve the main mono input/output buffers.
    pub fn main_mono_in_out<'a>(&'a mut self) -> MonoInOutStatus<'a, T> {
        let Self { main_in, main_out, .. } = self;

        if let Some(main_out) = main_out {
            if let Some(main_in) = main_in {
                return MonoInOutStatus::Separate {
                    input: main_in.mono(),
                    output: main_out.mono_mut(),
                };
            } else {
                return MonoInOutStatus::InPlace(main_out.mono_mut());
            }
        }

        MonoInOutStatus::NoMonoOut
    }

    /// A helper method to retrieve the main stereo input/output buffers.
    pub fn main_stereo_in_out<'a>(&'a mut self) -> StereoInOutStatus<'a, T> {
        let Self { main_in, main_out, .. } = self;

        if let Some(main_out) = main_out {
            if let Some(out_bufs) = main_out.stereo_mut() {
                if let Some(main_in) = main_in {
                    if let Some(in_bufs) = main_in.stereo() {
                        return StereoInOutStatus::Separate { input: in_bufs, output: out_bufs };
                    } else {
                        return StereoInOutStatus::InPlace(out_bufs);
                    }
                } else {
                    return StereoInOutStatus::InPlace(out_bufs);
                }
            }
        }

        StereoInOutStatus::NoStereoOut
    }

    /// A helper method to retrieve the main mono output buffer.
    #[inline]
    pub fn main_mono_out<'a>(&'a mut self) -> Option<&'a mut [T]> {
        if let Some(main_out) = &mut self.main_out {
            Some(main_out.mono_mut())
        } else {
            None
        }
    }

    /// A helper method to retrieve the main stereo output buffer.
    #[inline]
    pub fn main_stereo_out<'a>(&'a mut self) -> Option<(&'a mut [T], &'a mut [T])> {
        if let Some(main_out) = &mut self.main_out {
            if let Some(out_bufs) = main_out.stereo_mut() {
                return Some(out_bufs);
            }
        }

        None
    }

    /// Returns `true` if all the input buffers are silent.
    pub fn inputs_are_silent(&self) -> bool {
        // TODO

        false
    }
}

pub struct ProcInfo {
    /// A steady sample time counter.
    ///
    /// This field can be used to calculate the sleep duration between two process calls.
    /// This value may be specific to this plugin instance and have no relation to what
    /// other plugin instances may receive.
    ///
    /// This will return `None` if not available, otherwise the value will be increased by
    /// at least `frames_count` for the next call to process.
    pub steady_time: Option<Frames>,

    /// The number of frames to process.
    pub frames: Frames,
}
