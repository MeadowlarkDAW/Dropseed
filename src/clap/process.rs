use atomic_refcell::AtomicRefMut;
use clack_host::instance::processor::audio::{
    AudioBuffers, AudioPortBuffer as ClapAudioPortBuffer, AudioPortBufferType, AudioPorts,
    ChannelBuffer,
};
use std::ops::{Deref, DerefMut};

use crate::{
    plugin::{audio_buffer::RawAudioChannelBuffers, process_info::ProcBuffers},
    PluginAudioPortsExt,
};

// Deref coercion struggles to go from AtomicRefMut<Vec<T>> to [T]
struct BorrowedBuffer<'a, T>(AtomicRefMut<'a, Vec<T>>);

impl<'a, T> Deref for BorrowedBuffer<'a, T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.deref().deref()
    }
}

impl<'a, T> DerefMut for BorrowedBuffer<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut_slice()
    }
}

pub(crate) struct ClapProcess {
    input_buffer_slots: AudioPorts,
    output_buffer_slots: AudioPorts,
}

impl ClapProcess {
    pub fn new(audio_ports: &PluginAudioPortsExt) -> Self {
        // Allocate enough slots for each buffer so we can update them in
        // the audio thread.

        Self {
            input_buffer_slots: AudioPorts::with_capacity(2, audio_ports.inputs.len()),
            output_buffer_slots: AudioPorts::with_capacity(2, audio_ports.outputs.len()),
        }
    }

    pub fn update_buffers<'a>(
        &'a mut self,
        buffers: &'a ProcBuffers,
    ) -> (AudioBuffers<'a>, AudioBuffers<'a>) {
        debug_assert_eq!(buffers.audio_in.len(), self.input_buffer_slots.port_capacity());
        debug_assert_eq!(buffers.audio_out.len(), self.output_buffer_slots.port_capacity());

        let inputs = buffers.audio_in.iter().map(|port| ClapAudioPortBuffer {
            latency: port.latency(),
            channels: match &port.raw_channels {
                RawAudioChannelBuffers::F32(channels) => {
                    AudioPortBufferType::F32(channels.iter().map(|channel| ChannelBuffer {
                        data: BorrowedBuffer(channel.buffer.data.borrow_mut()),
                        is_constant: channel.is_constant(),
                    }))
                }
                RawAudioChannelBuffers::F64(channels) => {
                    AudioPortBufferType::F64(channels.iter().map(|channel| ChannelBuffer {
                        data: BorrowedBuffer(channel.buffer.data.borrow_mut()),
                        is_constant: channel.is_constant(),
                    }))
                }
            },
        });

        let outputs = buffers.audio_out.iter().map(|port| ClapAudioPortBuffer {
            latency: port.latency(),
            channels: match &port.raw_channels {
                RawAudioChannelBuffers::F32(channels) => {
                    AudioPortBufferType::F32(channels.iter().map(|channel| ChannelBuffer {
                        data: BorrowedBuffer(channel.buffer.data.borrow_mut()),
                        is_constant: channel.is_constant(),
                    }))
                }
                RawAudioChannelBuffers::F64(channels) => {
                    AudioPortBufferType::F64(channels.iter().map(|channel| ChannelBuffer {
                        data: BorrowedBuffer(channel.buffer.data.borrow_mut()),
                        is_constant: channel.is_constant(),
                    }))
                }
            },
        });

        (self.input_buffer_slots.with_data(inputs), self.output_buffer_slots.with_data(outputs))
    }
}
