use clap_sys::{
    audio_buffer::clap_audio_buffer,
    process::{
        clap_process, clap_process_status, CLAP_PROCESS_CONTINUE,
        CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_ERROR, CLAP_PROCESS_SLEEP,
    },
};
use rusty_daw_core::Frames;
use smallvec::SmallVec;

use crate::audio_buffer::{AudioPortBuffer, ClapAudioBuffer};

pub type PortID = u32;
pub type InPlacePairID = u32;

mod buffer_layout;
mod proc_audio_buffers;

pub use buffer_layout::{ProcBufferLayout, RawBufferLayout};
pub use proc_audio_buffers::ProcAudioBuffers;

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

/// The port buffers (for use with external CLAP plugins).
pub(crate) struct ClapProcAudioPorts {
    audio_inputs: SmallVec<[ClapAudioBuffer; 1]>,
    audio_outputs: SmallVec<[ClapAudioBuffer; 1]>,

    raw_audio_inputs: SmallVec<[*const clap_audio_buffer; 1]>,
    raw_audio_outputs: SmallVec<[*mut clap_audio_buffer; 1]>,

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
        let mut raw_audio_outputs: SmallVec<[*mut clap_audio_buffer; 1]> =
            SmallVec::with_capacity(audio_outputs.len());
        for _ in 0..audio_inputs.len() {
            raw_audio_inputs.push(std::ptr::null());
        }
        for _ in 0..audio_outputs.len() {
            raw_audio_outputs.push(std::ptr::null_mut());
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
                *self.raw_audio_outputs.get_unchecked_mut(i) =
                    self.audio_outputs[i].as_raw() as *mut clap_audio_buffer;
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
            std::ptr::null_mut()
        };

        proc.audio_inputs_count = self.audio_inputs_count;
        proc.audio_outputs_count = self.audio_outputs_count;
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
    pub frames: usize,
}
