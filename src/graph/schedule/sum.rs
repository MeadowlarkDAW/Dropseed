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
            let mut is_constant = true;
            for b in self.audio_in.iter() {
                if !b.is_constant() {
                    is_constant = false;
                    break;
                }
            }

            self.audio_out.set_constant(is_constant);

            let mut out_ref = self.audio_out.borrow_mut();

            #[cfg(debug_assertions)]
            let out = &mut out_ref[0..proc_info.frames];
            #[cfg(not(debug_assertions))]
            let out = std::slice::from_raw_parts_mut(out_ref.as_mut_ptr(), proc_info.frames);

            if is_constant {
                let total = self.audio_in.iter().fold(0.0, |acc, b| acc + b.borrow()[0]);
                out.fill(total);
                return;
            }

            // Unroll loops for common number of inputs.
            match self.audio_in.len() {
                0 => {
                    out.fill(0.0);
                }
                1 => {
                    let in_0_ref = self.audio_in[0].borrow();

                    #[cfg(debug_assertions)]
                    let in_0 = &in_0_ref[0..proc_info.frames];
                    #[cfg(not(debug_assertions))]
                    let in_0 = std::slice::from_raw_parts(in_0_ref.as_ptr(), proc_info.frames);

                    out.copy_from_slice(&in_0);
                }
                2 => {
                    let in_0_ref = self.audio_in[0].borrow();
                    let in_1_ref = self.audio_in[1].borrow();

                    #[cfg(debug_assertions)]
                    let in_0 = &in_0_ref[0..proc_info.frames];
                    #[cfg(debug_assertions)]
                    let in_1 = &in_1_ref[0..proc_info.frames];

                    #[cfg(not(debug_assertions))]
                    let in_0 = std::slice::from_raw_parts(in_0_ref.as_ptr(), proc_info.frames);
                    #[cfg(not(debug_assertions))]
                    let in_1 = std::slice::from_raw_parts(in_1_ref.as_ptr(), proc_info.frames);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i) + *in_1.get_unchecked(i);
                    }
                }
                3 => {
                    let in_0_ref = self.audio_in[0].borrow();
                    let in_1_ref = self.audio_in[1].borrow();
                    let in_2_ref = self.audio_in[2].borrow();

                    #[cfg(debug_assertions)]
                    let in_0 = &in_0_ref[0..proc_info.frames];
                    #[cfg(debug_assertions)]
                    let in_1 = &in_1_ref[0..proc_info.frames];
                    #[cfg(debug_assertions)]
                    let in_2 = &in_2_ref[0..proc_info.frames];

                    #[cfg(not(debug_assertions))]
                    let in_0 = std::slice::from_raw_parts(in_0_ref.as_ptr(), proc_info.frames);
                    #[cfg(not(debug_assertions))]
                    let in_1 = std::slice::from_raw_parts(in_1_ref.as_ptr(), proc_info.frames);
                    #[cfg(not(debug_assertions))]
                    let in_2 = std::slice::from_raw_parts(in_2_ref.as_ptr(), proc_info.frames);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i);
                    }
                }
                4 => {
                    let in_0_ref = self.audio_in[0].borrow();
                    let in_1_ref = self.audio_in[1].borrow();
                    let in_2_ref = self.audio_in[2].borrow();
                    let in_3_ref = self.audio_in[3].borrow();

                    #[cfg(debug_assertions)]
                    let in_0 = &in_0_ref[0..proc_info.frames];
                    #[cfg(debug_assertions)]
                    let in_1 = &in_1_ref[0..proc_info.frames];
                    #[cfg(debug_assertions)]
                    let in_2 = &in_2_ref[0..proc_info.frames];
                    #[cfg(debug_assertions)]
                    let in_3 = &in_3_ref[0..proc_info.frames];

                    #[cfg(not(debug_assertions))]
                    let in_0 = std::slice::from_raw_parts(in_0_ref.as_ptr(), proc_info.frames);
                    #[cfg(not(debug_assertions))]
                    let in_1 = std::slice::from_raw_parts(in_1_ref.as_ptr(), proc_info.frames);
                    #[cfg(not(debug_assertions))]
                    let in_2 = std::slice::from_raw_parts(in_2_ref.as_ptr(), proc_info.frames);
                    #[cfg(not(debug_assertions))]
                    let in_3 = std::slice::from_raw_parts(in_3_ref.as_ptr(), proc_info.frames);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i)
                            + *in_3.get_unchecked(i);
                    }
                }
                num_inputs => {
                    let in_0_ref = self.audio_in[0].borrow();

                    #[cfg(debug_assertions)]
                    let in_0 = &in_0_ref[0..proc_info.frames];
                    #[cfg(not(debug_assertions))]
                    let in_0 = std::slice::from_raw_parts(in_0_ref.as_ptr(), proc_info.frames);

                    out.copy_from_slice(in_0);

                    for ch_i in 1..num_inputs {
                        let input_ref = self.audio_in[ch_i].borrow();

                        #[cfg(debug_assertions)]
                        let input = &input_ref[0..proc_info.frames];
                        #[cfg(not(debug_assertions))]
                        let input =
                            std::slice::from_raw_parts(input_ref.as_ptr(), proc_info.frames);

                        for smp_i in 0..proc_info.frames {
                            *out.get_unchecked_mut(smp_i) += *input.get_unchecked(smp_i);
                        }
                    }
                }
            }
        }
    }
}
