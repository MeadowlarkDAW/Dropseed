use smallvec::SmallVec;

use crate::{Host, ProcInfo};

use super::audio_buffer_pool::SharedAudioBuffer;

pub(crate) mod delay_comp_node;
pub(crate) mod task;

use task::Task;

pub struct Schedule {
    pub(crate) tasks: Vec<Task>,

    pub(crate) graph_inputs: SmallVec<[SharedAudioBuffer<f32>; 4]>,
    pub(crate) graph_outputs: SmallVec<[SharedAudioBuffer<f32>; 4]>,

    pub(crate) max_block_size: usize,

    /// Used to get info and request actions from the host.
    pub(crate) host: Host,
}

impl Schedule {
    #[cfg(feature = "cpal-backend")]
    pub fn process_cpal_output_interleaved<T: cpal::Sample>(
        &mut self,
        num_out_channels: usize,
        out: &mut [T],
    ) {
        if num_out_channels == 0 || out.len() == 0 {
            for smp in out.iter_mut() {
                *smp = T::from(&0.0);
            }

            return;
        }

        // Get the number of frames in this process cycle
        let total_frames = out.len() / num_out_channels;

        if total_frames * num_out_channels != out.len() {
            log::warn!("The given cpal output buffer with {} total samples is not a multiple of {} channels", out.len(), num_out_channels);
            for smp in out[(total_frames * num_out_channels)..].iter_mut() {
                *smp = T::from(&0.0);
            }
        }

        let mut processed_frames = 0;
        while processed_frames < total_frames {
            let frames = (total_frames - processed_frames).min(self.max_block_size);

            let proc_info = ProcInfo {
                steady_time: None, // TODO
                frames,
            };

            // We are ignoring sytem inputs with the CPAL backend for now.
            for buffer in self.graph_inputs.iter() {
                let buffer = buffer.borrow_mut(&proc_info);
                buffer.fill(0.0);
            }

            for task in self.tasks.iter_mut() {
                task.process(&proc_info, &mut self.host)
            }

            let out_part = &mut out[(processed_frames * num_out_channels)
                ..((processed_frames + frames) * num_out_channels)];
            for ch_i in 0..num_out_channels {
                if let Some(buffer) = self.graph_outputs.get(ch_i) {
                    let buffer = buffer.borrow(&proc_info);

                    for i in 0..frames {
                        // TODO: Optimize with unsafe bounds checking?
                        out_part[(i * num_out_channels) + ch_i] = T::from(&buffer[i]);
                    }
                } else {
                    for i in 0..frames {
                        // TODO: Optimize with unsafe bounds checking?
                        out_part[(i * num_out_channels) + ch_i] = T::from(&0.0);
                    }
                }
            }

            processed_frames += frames;
        }
    }
}
