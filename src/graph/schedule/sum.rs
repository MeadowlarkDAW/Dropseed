use smallvec::SmallVec;

use crate::graph::shared_pool::SharedBuffer;
use crate::plugin::process_info::ProcInfo;

pub(crate) struct SumTask {
    pub audio_in: SmallVec<[SharedBuffer<f32>; 4]>,
    pub audio_out: SharedBuffer<f32>,
}

impl SumTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let mut is_constant = true;
        for b in self.audio_in.iter() {
            if !b.is_constant() {
                is_constant = false;
                break;
            }
        }

        self.audio_out.set_constant(is_constant);

        let mut out_ref = self.audio_out.borrow_mut();

        let out = &mut out_ref[0..proc_info.frames];

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

                let in_0 = &in_0_ref[0..proc_info.frames];

                out.copy_from_slice(in_0);
            }
            2 => {
                let in_0_ref = self.audio_in[0].borrow();
                let in_1_ref = self.audio_in[1].borrow();

                let in_0 = &in_0_ref[0..proc_info.frames];
                let in_1 = &in_1_ref[0..proc_info.frames];

                for i in 0..proc_info.frames {
                    out[i] = in_0[i] + in_1[i];
                }
            }
            3 => {
                let in_0_ref = self.audio_in[0].borrow();
                let in_1_ref = self.audio_in[1].borrow();
                let in_2_ref = self.audio_in[2].borrow();

                let in_0 = &in_0_ref[0..proc_info.frames];
                let in_1 = &in_1_ref[0..proc_info.frames];
                let in_2 = &in_2_ref[0..proc_info.frames];

                for i in 0..proc_info.frames {
                    out[i] = in_0[i] + in_1[i] + in_2[i];
                }
            }
            4 => {
                let in_0_ref = self.audio_in[0].borrow();
                let in_1_ref = self.audio_in[1].borrow();
                let in_2_ref = self.audio_in[2].borrow();
                let in_3_ref = self.audio_in[3].borrow();

                let in_0 = &in_0_ref[0..proc_info.frames];
                let in_1 = &in_1_ref[0..proc_info.frames];
                let in_2 = &in_2_ref[0..proc_info.frames];
                let in_3 = &in_3_ref[0..proc_info.frames];

                for i in 0..proc_info.frames {
                    out[i] = in_0[i] + in_1[i] + in_2[i] + in_3[i];
                }
            }
            num_inputs => {
                let in_0_ref = self.audio_in[0].borrow();

                let in_0 = &in_0_ref[0..proc_info.frames];

                out.copy_from_slice(in_0);

                for ch_i in 1..num_inputs {
                    let input_ref = self.audio_in[ch_i].borrow();

                    let input = &input_ref[0..proc_info.frames];

                    for smp_i in 0..proc_info.frames {
                        out[smp_i] += input[smp_i];
                    }
                }
            }
        }
    }
}
