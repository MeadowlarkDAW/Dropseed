use atomic_refcell::AtomicRefMut;
use clack_host::instance::processor::audio::{
    AudioPortBuffer as ClapAudioPortBuffer, AudioPortBufferType, AudioPorts, InputAudioBuffers,
    InputChannel, OutputAudioBuffers,
};
use smallvec::SmallVec;

use dropseed_plugin_api::buffer::{BufferInner, RawAudioChannelBuffers};
use dropseed_plugin_api::ProcBuffers;

use super::plugin::AudioPortChannels;

pub(crate) struct ClapAudioPorts {
    pub(crate) input_buffer_slots: AudioPorts,
    pub(crate) output_buffer_slots: AudioPorts,
}

type BorrowedChannelsRefsMut<'a, T> = SmallVec<[AtomicRefMut<'a, BufferInner<T>>; 4]>;

enum BorrowedPortMut<'a> {
    F32(BorrowedChannelsRefsMut<'a, f32>),
    F64(BorrowedChannelsRefsMut<'a, f64>),
}

impl<'a> BorrowedPortMut<'a> {
    fn borrow(buf: &'a RawAudioChannelBuffers) -> Self {
        match buf {
            RawAudioChannelBuffers::F32(channels) => {
                Self::F32(channels.iter().map(|c| c.borrow_mut()).collect())
            }
            RawAudioChannelBuffers::F64(channels) => {
                Self::F64(channels.iter().map(|c| c.borrow_mut()).collect())
            }
        }
    }
}

type BorrowedPortsRefsMut<'a> = SmallVec<[(BorrowedPortMut<'a>, u32); 8]>;

pub(crate) struct BorrowedClapAudioBuffers<'a> {
    inputs: BorrowedPortsRefsMut<'a>,
    outputs: BorrowedPortsRefsMut<'a>,
}

impl<'a> BorrowedClapAudioBuffers<'a> {
    pub fn borrow_all(buffers: &'a ProcBuffers) -> Self {
        Self {
            inputs: buffers
                .audio_in
                .iter()
                .map(|p| (BorrowedPortMut::borrow(&p._raw_channels), p.latency()))
                .collect(),
            outputs: buffers
                .audio_out
                .iter()
                .map(|p| (BorrowedPortMut::borrow(&p._raw_channels), p.latency()))
                .collect(),
        }
    }
}

impl ClapAudioPorts {
    pub(super) fn new(audio_port_channels: &AudioPortChannels) -> Self {
        // Allocate enough slots for each buffer so we can update them in
        // the audio thread.
        Self {
            input_buffer_slots: AudioPorts::with_capacity(
                audio_port_channels.max_input_channels,
                audio_port_channels.num_input_ports,
            ),
            output_buffer_slots: AudioPorts::with_capacity(
                audio_port_channels.max_output_channels,
                audio_port_channels.num_output_ports,
            ),
        }
    }

    pub fn create_inout_buffers<'s, 'b: 's>(
        &'s mut self,
        buffers: &'s mut BorrowedClapAudioBuffers<'b>,
    ) -> (InputAudioBuffers<'s>, OutputAudioBuffers<'s>) {
        debug_assert_eq!(buffers.inputs.len(), self.input_buffer_slots.port_capacity());
        debug_assert_eq!(buffers.outputs.len(), self.output_buffer_slots.port_capacity());

        let inputs = buffers.inputs.iter_mut().map(|port| ClapAudioPortBuffer {
            latency: port.1,
            channels: match &mut port.0 {
                BorrowedPortMut::F32(channels) => {
                    AudioPortBufferType::F32(channels.iter_mut().map(|channel| {
                        let is_constant = channel.is_constant;
                        InputChannel::from_buffer(&mut channel.data, is_constant)
                    }))
                }
                BorrowedPortMut::F64(channels) => {
                    AudioPortBufferType::F64(channels.iter_mut().map(|channel| {
                        let is_constant = channel.is_constant;
                        InputChannel::from_buffer(&mut channel.data, is_constant)
                    }))
                }
            },
        });

        let outputs = buffers.outputs.iter_mut().map(|port| ClapAudioPortBuffer {
            latency: port.1,
            channels: match &mut port.0 {
                BorrowedPortMut::F32(channels) => AudioPortBufferType::F32(
                    channels.iter_mut().map(|channel| channel.data.as_mut_slice()),
                ),
                BorrowedPortMut::F64(channels) => AudioPortBufferType::F64(
                    channels.iter_mut().map(|channel| channel.data.as_mut_slice()),
                ),
            },
        });

        (
            self.input_buffer_slots.with_input_buffers(inputs),
            self.output_buffer_slots.with_output_buffers(outputs),
        )
    }

    pub fn update_output_constant_flags(&mut self, buffers: &mut BorrowedClapAudioBuffers) {
        for (info, port) in self.output_buffer_slots.port_infos().zip(buffers.outputs.iter_mut()) {
            match &mut port.0 {
                BorrowedPortMut::F32(b) => {
                    for (channel, is_constant) in b.iter_mut().zip(info.channel_constants()) {
                        channel.is_constant = is_constant
                    }
                }
                BorrowedPortMut::F64(b) => {
                    for (channel, is_constant) in b.iter_mut().zip(info.channel_constants()) {
                        channel.is_constant = is_constant
                    }
                }
            }
        }
    }
}
