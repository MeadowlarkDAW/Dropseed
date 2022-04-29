use clap_sys::audio_buffer::clap_audio_buffer as RawClapAudioBuffer;
use clap_sys::process::clap_process as RawClapProcess;
use smallvec::SmallVec;
use std::{pin::Pin, ptr};

use crate::{graph::audio_buffer_pool::AudioPortBuffer, plugin::process_info::ProcInfo};

pub(crate) struct ClapProcess {
    raw: RawClapProcess,

    audio_in: SmallVec<[AudioPortBuffer; 2]>,
    audio_out: SmallVec<[AudioPortBuffer; 2]>,

    audio_in_list: Pin<SmallVec<[RawClapAudioBuffer; 2]>>,
    audio_out_list: Pin<SmallVec<[RawClapAudioBuffer; 2]>>,

    audio_in_buffer_lists: SmallVec<[Pin<SmallVec<[*const f32; 2]>>; 2]>,
    audio_out_buffer_lists: SmallVec<[Pin<SmallVec<[*const f32; 2]>>; 2]>,
}

impl ClapProcess {
    pub fn new(
        &mut self,
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
        let (audio_in_list, audio_out_list, audio_in_buffer_lists, audio_out_buffer_lists) = unsafe {
            let audio_in_buffer_lists: SmallVec<[Pin<SmallVec<[*const f32; 2]>>; 2]> = audio_in
                .iter()
                .map(|b| Pin::new(b.rc_buffers().iter().map(|rc_b| rc_b.raw_buffer()).collect()))
                .collect();

            let audio_out_buffer_lists: SmallVec<[Pin<SmallVec<[*const f32; 2]>>; 2]> = audio_out
                .iter()
                .map(|b| Pin::new(b.rc_buffers().iter().map(|rc_b| rc_b.raw_buffer()).collect()))
                .collect();

            let audio_in_list: SmallVec<[RawClapAudioBuffer; 2]> = audio_in
                .iter()
                .map(|b| RawClapAudioBuffer {
                    data32: ptr::null(),
                    data64: ptr::null(),
                    channel_count: b.channel_count() as u32,
                    latency: b.latency(),
                    constant_mask: b.silent_mask(),
                })
                .collect();

            let audio_out_list: SmallVec<[RawClapAudioBuffer; 2]> = audio_out
                .iter()
                .map(|b| RawClapAudioBuffer {
                    data32: ptr::null(),
                    data64: ptr::null(),
                    channel_count: b.channel_count() as u32,
                    latency: b.latency(),
                    constant_mask: b.silent_mask(),
                })
                .collect();

            (audio_in_list, audio_out_list, audio_in_buffer_lists, audio_out_buffer_lists)
        };

        let mut new_self = Self {
            raw: RawClapProcess {
                steady_time: -1,
                frames_count: 0,
                transport: ptr::null(),
                audio_inputs: ptr::null(),
                audio_outputs: ptr::null_mut(),
                audio_inputs_count: audio_in.len() as u32,
                audio_outputs_count: audio_out.len() as u32,
                in_events: ptr::null(),
                out_events: ptr::null(),
            },
            audio_in,
            audio_out,
            audio_in_list: Pin::new(audio_in_list),
            audio_out_list: Pin::new(audio_out_list),
            audio_in_buffer_lists,
            audio_out_buffer_lists,
        };

        for (port_buf, raw_bufs) in
            new_self.audio_in_list.iter_mut().zip(new_self.audio_in_buffer_lists.iter())
        {
            port_buf.data32 = raw_bufs.as_ptr();
        }
        for (port_buf, raw_bufs) in
            new_self.audio_out_list.iter_mut().zip(new_self.audio_out_buffer_lists.iter())
        {
            port_buf.data32 = raw_bufs.as_ptr();
        }

        new_self.raw.audio_inputs = new_self.audio_in_list.as_ptr();
        new_self.raw.audio_outputs = new_self.audio_out_list.as_mut_ptr();

        new_self
    }

    pub fn update_frames(&mut self, proc_info: &ProcInfo) {
        self.raw.steady_time = proc_info.steady_time.unwrap_or(-1);
        self.raw.frames_count = proc_info.frames as u32;
    }

    pub fn raw(&self) -> &RawClapProcess {
        &self.raw
    }
}
