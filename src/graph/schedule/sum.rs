use smallvec::SmallVec;

use crate::graph::shared_pool::SharedBuffer;
use crate::plugin::process_info::ProcInfo;

pub(crate) struct SumTask {
    pub audio_in: SmallVec<[SharedBuffer<f32>; 4]>,
    pub audio_out: SharedBuffer<f32>,
}

impl SumTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // SAFETY
        // - These buffers are only ever borrowed in the audio thread.
        // - The schedule verifier has ensured that no data races can occur between parallel
        // audio threads due to aliasing buffer pointers.
        // - `proc_info.frames` will always be less than or equal to the allocated size of
        // all process audio buffers.
        unsafe {
            let out = self.audio_out.slice_from_frames_unchecked_mut(proc_info.frames);

            // Unroll loops for common number of inputs.
            match self.audio_in.len() {
                0 => return,
                1 => {
                    let in_0 = self.audio_in[0].slice_from_frames_unchecked(proc_info.frames);
                    out.copy_from_slice(&in_0);
                }
                2 => {
                    let in_0 = self.audio_in[0].slice_from_frames_unchecked(proc_info.frames);
                    let in_1 = self.audio_in[1].slice_from_frames_unchecked(proc_info.frames);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i) + *in_1.get_unchecked(i);
                    }
                }
                3 => {
                    let in_0 = self.audio_in[0].slice_from_frames_unchecked(proc_info.frames);
                    let in_1 = self.audio_in[1].slice_from_frames_unchecked(proc_info.frames);
                    let in_2 = self.audio_in[2].slice_from_frames_unchecked(proc_info.frames);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i);
                    }
                }
                4 => {
                    let in_0 = self.audio_in[0].slice_from_frames_unchecked(proc_info.frames);
                    let in_1 = self.audio_in[1].slice_from_frames_unchecked(proc_info.frames);
                    let in_2 = self.audio_in[2].slice_from_frames_unchecked(proc_info.frames);
                    let in_3 = self.audio_in[3].slice_from_frames_unchecked(proc_info.frames);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i)
                            + *in_3.get_unchecked(i);
                    }
                }
                num_inputs => {
                    let in_0 = self.audio_in[0].slice_from_frames_unchecked(proc_info.frames);

                    out.copy_from_slice(in_0);

                    for ch_i in 1..num_inputs {
                        let input =
                            self.audio_in[ch_i].slice_from_frames_unchecked(proc_info.frames);
                        for smp_i in 0..proc_info.frames {
                            *out.get_unchecked_mut(smp_i) += *input.get_unchecked(smp_i);
                        }
                    }
                }
            }
        }
    }
}
