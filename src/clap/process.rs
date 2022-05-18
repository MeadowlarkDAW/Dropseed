use clap_sys::audio_buffer::clap_audio_buffer as RawClapAudioBuffer;
use clap_sys::process::clap_process as RawClapProcess;
use smallvec::SmallVec;
use std::{pin::Pin, ptr};

use crate::{graph::audio_buffer_pool::AudioPortBuffer, plugin::process_info::ProcInfo};

use super::events::{ClapInputEvents, ClapOutputEvents};

pub(crate) struct ClapProcess {
    raw: RawClapProcess,

    pub audio_in: SmallVec<[AudioPortBuffer; 2]>,
    pub audio_out: SmallVec<[AudioPortBuffer; 2]>,

    _audio_in_list: Pin<Vec<RawClapAudioBuffer>>,
    _audio_out_list: Pin<Vec<RawClapAudioBuffer>>,

    _audio_in_buffer_lists: SmallVec<[Pin<Vec<*const f32>>; 2]>,
    _audio_out_buffer_lists: SmallVec<[Pin<Vec<*const f32>>; 2]>,

    _in_events: Pin<Box<ClapInputEvents>>,
    _out_events: Pin<Box<ClapOutputEvents>>,
}

impl ClapProcess {
    pub fn new(
        audio_in: SmallVec<[AudioPortBuffer; 2]>,
        audio_out: SmallVec<[AudioPortBuffer; 2]>,
    ) -> Self {
        // Please refer to the "SAFETY NOTE" at the top of the file
        // `src/graph/audio_buffer_pool.rs` on why it is considered safe to
        // borrow these buffers.
        //
        // Also, we are pinning the list of buffers in place, so the pointers
        // in the raw clap_process struct will always point to valid data.
        //
        // In addition, this struct will hold on to the SharedAudioBuffers
        // themselves, ensuring that these buffers will never be deallocated
        // for as long as this struct lives.
        let (audio_in_list, mut audio_out_list, audio_in_buffer_lists, audio_out_buffer_lists) = unsafe {
            let audio_in_buffer_lists: SmallVec<[Pin<Vec<*const f32>>; 2]> = audio_in
                .iter()
                .map(|b| Pin::new(b.rc_buffers().iter().map(|rc_b| rc_b.raw_buffer()).collect()))
                .collect();

            let audio_out_buffer_lists: SmallVec<[Pin<Vec<*const f32>>; 2]> = audio_out
                .iter()
                .map(|b| Pin::new(b.rc_buffers().iter().map(|rc_b| rc_b.raw_buffer()).collect()))
                .collect();

            let audio_in_list: Pin<Vec<RawClapAudioBuffer>> = Pin::new(
                audio_in
                    .iter()
                    .zip(audio_in_buffer_lists.iter())
                    .map(|(b, raw_b)| RawClapAudioBuffer {
                        data32: raw_b.as_ptr(),
                        data64: ptr::null(),
                        channel_count: b.channel_count() as u32,
                        latency: b.latency(),
                        constant_mask: b.silent_mask(),
                    })
                    .collect(),
            );

            let audio_out_list: Pin<Vec<RawClapAudioBuffer>> = Pin::new(
                audio_out
                    .iter()
                    .zip(audio_out_buffer_lists.iter())
                    .map(|(b, raw_b)| RawClapAudioBuffer {
                        data32: raw_b.as_ptr(),
                        data64: ptr::null(),
                        channel_count: b.channel_count() as u32,
                        latency: b.latency(),
                        constant_mask: b.silent_mask(),
                    })
                    .collect(),
            );

            (audio_in_list, audio_out_list, audio_in_buffer_lists, audio_out_buffer_lists)
        };

        let in_events = Pin::new(Box::new(ClapInputEvents::new()));
        let out_events = Pin::new(Box::new(ClapOutputEvents::new()));

        Self {
            raw: RawClapProcess {
                steady_time: -1,
                frames_count: 0,
                transport: ptr::null(),
                audio_inputs: audio_in_list.as_ptr(),
                audio_outputs: audio_out_list.as_mut_ptr(),
                audio_inputs_count: audio_in.len() as u32,
                audio_outputs_count: audio_out.len() as u32,
                in_events: in_events.raw(),
                out_events: out_events.raw(),
            },
            audio_in,
            audio_out,
            _audio_in_list: audio_in_list,
            _audio_out_list: audio_out_list,
            _audio_in_buffer_lists: audio_in_buffer_lists,
            _audio_out_buffer_lists: audio_out_buffer_lists,
            _in_events: in_events,
            _out_events: out_events,
        }
    }

    pub fn update_frames(&mut self, proc_info: &ProcInfo) {
        self.raw.steady_time = proc_info.steady_time.unwrap_or(-1);
        self.raw.frames_count = proc_info.frames as u32;
    }

    pub fn raw(&self) -> &RawClapProcess {
        &self.raw
    }
}
