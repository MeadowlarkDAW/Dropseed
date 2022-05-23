use clap_sys::audio_buffer::clap_audio_buffer as RawClapAudioBuffer;
use clap_sys::process::clap_process as RawClapProcess;
use smallvec::SmallVec;
use std::{pin::Pin, ptr};

use crate::{
    plugin::process_info::ProcInfo,
    plugin::{audio_buffer::RawAudioChannelBuffers, process_info::ProcBuffers},
    AudioPortsExtension,
};

use super::events::{ClapInputEvents, ClapOutputEvents};

pub(crate) struct ClapProcess {
    raw: RawClapProcess,

    audio_in_port_list: Pin<Vec<RawClapAudioBuffer>>,
    audio_out_port_list: Pin<Vec<RawClapAudioBuffer>>,

    audio_in_buffer_lists_f32: SmallVec<[Pin<Vec<*const f32>>; 2]>,
    audio_out_buffer_lists_f32: SmallVec<[Pin<Vec<*const f32>>; 2]>,

    audio_in_buffer_lists_f64: SmallVec<[Pin<Vec<*const f64>>; 2]>,
    audio_out_buffer_lists_f64: SmallVec<[Pin<Vec<*const f64>>; 2]>,

    _in_events: Pin<Box<ClapInputEvents>>,
    _out_events: Pin<Box<ClapOutputEvents>>,
}

unsafe impl Send for ClapProcess {}
unsafe impl Sync for ClapProcess {}

impl ClapProcess {
    pub fn new(audio_ports: &AudioPortsExtension) -> Self {
        // Allocate enough slots for each buffer so we can update them in
        // the audio thread.

        let mut audio_in_port_list: Vec<RawClapAudioBuffer> =
            Vec::with_capacity(audio_ports.inputs.len());
        let mut audio_out_port_list: Vec<RawClapAudioBuffer> =
            Vec::with_capacity(audio_ports.outputs.len());

        let mut audio_in_buffer_lists_f32: SmallVec<[Pin<Vec<*const f32>>; 2]> =
            SmallVec::with_capacity(audio_ports.inputs.len());
        let mut audio_out_buffer_lists_f32: SmallVec<[Pin<Vec<*const f32>>; 2]> =
            SmallVec::with_capacity(audio_ports.outputs.len());

        let mut audio_in_buffer_lists_f64: SmallVec<[Pin<Vec<*const f64>>; 2]> =
            SmallVec::with_capacity(audio_ports.inputs.len());
        let mut audio_out_buffer_lists_f64: SmallVec<[Pin<Vec<*const f64>>; 2]> =
            SmallVec::with_capacity(audio_ports.outputs.len());

        for in_port in audio_ports.inputs.iter() {
            audio_in_port_list.push(RawClapAudioBuffer {
                data32: ptr::null(),
                data64: ptr::null(),
                channel_count: in_port.channels as u32,
                latency: 0,
                constant_mask: 0,
            });

            let buffers_f32: Vec<*const f32> = (0..in_port.channels).map(|_| ptr::null()).collect();
            let buffers_f64: Vec<*const f64> = (0..in_port.channels).map(|_| ptr::null()).collect();

            audio_in_buffer_lists_f32.push(Pin::new(buffers_f32));
            audio_in_buffer_lists_f64.push(Pin::new(buffers_f64));
        }
        for out_port in audio_ports.outputs.iter() {
            audio_out_port_list.push(RawClapAudioBuffer {
                data32: ptr::null(),
                data64: ptr::null(),
                channel_count: out_port.channels as u32,
                latency: 0,
                constant_mask: 0,
            });

            let buffers_f32: Vec<*const f32> =
                (0..out_port.channels).map(|_| ptr::null()).collect();
            let buffers_f64: Vec<*const f64> =
                (0..out_port.channels).map(|_| ptr::null()).collect();

            audio_out_buffer_lists_f32.push(Pin::new(buffers_f32));
            audio_out_buffer_lists_f64.push(Pin::new(buffers_f64));
        }

        let audio_in_port_list = Pin::new(audio_in_port_list);
        let mut audio_out_port_list = Pin::new(audio_out_port_list);

        let _in_events = Pin::new(Box::new(ClapInputEvents::new()));
        let _out_events = Pin::new(Box::new(ClapOutputEvents::new()));

        Self {
            raw: RawClapProcess {
                steady_time: -1,
                frames_count: 0,
                transport: ptr::null(),
                audio_inputs: (*audio_in_port_list).as_ptr(),
                audio_outputs: (*audio_out_port_list).as_mut_ptr(),
                audio_inputs_count: audio_in_port_list.len() as u32,
                audio_outputs_count: audio_out_port_list.len() as u32,
                in_events: _in_events.raw(),
                out_events: _out_events.raw(),
            },
            audio_in_port_list,
            audio_out_port_list,
            audio_in_buffer_lists_f32,
            audio_out_buffer_lists_f32,
            audio_in_buffer_lists_f64,
            audio_out_buffer_lists_f64,
            _in_events,
            _out_events,
        }
    }

    pub fn update_buffers(&mut self, buffers: &ProcBuffers) {
        debug_assert_eq!(buffers.audio_in.len(), self.audio_in_port_list.len());
        debug_assert_eq!(buffers.audio_out.len(), self.audio_out_port_list.len());

        unsafe {
            for i in 0..buffers.audio_in.len() {
                let new_in_port = buffers.audio_in.get_unchecked(i);
                let in_port = self.audio_in_port_list.get_unchecked_mut(i);

                in_port.latency = new_in_port.latency();

                match &new_in_port.raw_channels {
                    RawAudioChannelBuffers::F32(new_buffers) => {
                        let buffers = self.audio_in_buffer_lists_f32.get_unchecked_mut(i);

                        debug_assert_eq!(new_buffers.len(), buffers.len());

                        for (buf, new_buf) in buffers.iter_mut().zip(new_buffers.iter()) {
                            *buf = (*new_buf.buffer.0.get()).as_ptr();
                        }

                        in_port.data32 = buffers.as_ptr();
                        in_port.data64 = ptr::null();
                    }
                    RawAudioChannelBuffers::F64(new_buffers) => {
                        let buffers = self.audio_in_buffer_lists_f64.get_unchecked_mut(i);

                        debug_assert_eq!(new_buffers.len(), buffers.len());

                        for (buf, new_buf) in buffers.iter_mut().zip(new_buffers.iter()) {
                            *buf = (*new_buf.buffer.0.get()).as_ptr();
                        }

                        in_port.data32 = ptr::null();
                        in_port.data64 = buffers.as_ptr();
                    }
                }
            }

            for i in 0..buffers.audio_out.len() {
                let new_out_port = buffers.audio_out.get_unchecked(i);
                let out_port = self.audio_out_port_list.get_unchecked_mut(i);

                out_port.latency = new_out_port.latency();

                match &new_out_port.raw_channels {
                    RawAudioChannelBuffers::F32(new_buffers) => {
                        let buffers = self.audio_out_buffer_lists_f32.get_unchecked_mut(i);

                        debug_assert_eq!(new_buffers.len(), buffers.len());

                        for (buf, new_buf) in buffers.iter_mut().zip(new_buffers.iter()) {
                            *buf = (*new_buf.buffer.0.get()).as_ptr();
                        }

                        out_port.data32 = buffers.as_ptr();
                        out_port.data64 = ptr::null();
                    }
                    RawAudioChannelBuffers::F64(new_buffers) => {
                        let buffers = self.audio_out_buffer_lists_f64.get_unchecked_mut(i);

                        debug_assert_eq!(new_buffers.len(), buffers.len());

                        for (buf, new_buf) in buffers.iter_mut().zip(new_buffers.iter()) {
                            *buf = (*new_buf.buffer.0.get()).as_ptr();
                        }

                        out_port.data32 = ptr::null();
                        out_port.data64 = buffers.as_ptr();
                    }
                }
            }
        }
    }

    pub fn sync_proc_info(&mut self, proc_info: &ProcInfo, buffers: &ProcBuffers) {
        self.raw.steady_time = proc_info.steady_time;
        self.raw.frames_count = proc_info.frames as u32;

        debug_assert_eq!(buffers.audio_in.len(), self.audio_in_port_list.len());
        for (audio_in_port, host_audio_in_port) in
            self.audio_in_port_list.iter_mut().zip(buffers.audio_in.iter())
        {
            audio_in_port.constant_mask = host_audio_in_port.constant_mask();
        }
    }

    pub fn raw(&self) -> *const RawClapProcess {
        &self.raw
    }

    pub fn sync_output_constant_masks(&mut self, buffers: &ProcBuffers) {
        debug_assert_eq!(buffers.audio_out.len(), self.audio_out_port_list.len());
        for (audio_out_port, host_audio_out_port) in
            self.audio_out_port_list.iter().zip(buffers.audio_out.iter())
        {
            host_audio_out_port.set_constant_mask(audio_out_port.constant_mask);
        }
    }
}
