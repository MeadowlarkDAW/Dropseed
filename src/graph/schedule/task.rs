use smallvec::SmallVec;

use crate::graph::audio_buffer_pool::SharedAudioBuffer;
use crate::graph::plugin_pool::{SharedDelayCompNode, SharedPluginAudioThreadInstance};
use crate::{AudioPortBuffer, ProcInfo, ProcessStatus};

#[cfg(feature = "clap-host")]
use crate::clap::task::ClapPluginTask;

pub(crate) enum Task {
    InternalPlugin(InternalPluginTask),

    #[cfg(feature = "clap-host")]
    ClapPlugin(ClapPluginTask),

    DelayComp(DelayCompTask),
    Sum(SumTask),
    DeactivatedPlugin(DeactivatedPluginTask),
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::InternalPlugin(t) => {
                let mut f = f.debug_struct("InternalPlugin");

                f.field("id", &t.plugin.id());

                if !t.audio_in.is_empty() {
                    let mut s = String::new();
                    for b in t.audio_in.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_in", &s);
                }

                if !t.audio_out.is_empty() {
                    let mut s = String::new();
                    for b in t.audio_out.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_out", &s);
                }

                f.finish()
            }
            #[cfg(feature = "clap-host")]
            Task::ClapPlugin(t) => {
                let mut f = f.debug_struct("ClapPlugin");

                // TODO: Processor ID

                //t.ports.debug_fields(&mut f);

                f.finish()
            }
            Task::DelayComp(t) => {
                let mut f = f.debug_struct("DelayComp");

                f.field("audio_in", &t.audio_in.unique_id());
                f.field("audio_out", &t.audio_out.unique_id());
                f.field("delay", &t.delay_comp_node.delay());

                f.finish()
            }
            Task::Sum(t) => {
                let mut f = f.debug_struct("Sum");

                let mut s = String::new();
                for b in t.audio_in.iter() {
                    s.push_str(&format!("{:?}, ", b.unique_id()))
                }
                f.field("audio_in", &s);

                f.field("audio_out", &format!("{:?}", t.audio_out.unique_id()));

                f.finish()
            }
            Task::DeactivatedPlugin(t) => {
                let mut f = f.debug_struct("DeactivatedPlugin");

                let mut s = String::new();
                for (b_in, b_out) in t.audio_through.iter() {
                    s.push_str(&format!(
                        "(in: {:?}, out: {:?})",
                        b_in.unique_id(),
                        b_out.unique_id()
                    ));
                }
                f.field("audio_through", &s);

                let mut s = String::new();
                for b in t.extra_audio_out.iter() {
                    s.push_str(&format!("{:?}, ", b.unique_id()))
                }
                f.field("extra_audio_out", &s);

                f.finish()
            }
        }
    }
}

impl Task {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        match self {
            Task::InternalPlugin(task) => task.process(proc_info),
            #[cfg(feature = "clap-host")]
            Task::ClapPlugin(task) => {
                todo!()
            }
            Task::DelayComp(task) => task.process(proc_info),
            Task::Sum(task) => task.process(proc_info),
            Task::DeactivatedPlugin(task) => task.process(proc_info),
        }
    }
}

pub(crate) struct InternalPluginTask {
    pub plugin: SharedPluginAudioThreadInstance,

    pub audio_in: SmallVec<[AudioPortBuffer; 2]>,
    pub audio_out: SmallVec<[AudioPortBuffer; 2]>,
}

impl InternalPluginTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        let Self { plugin, audio_in, audio_out } = self;

        // This is safe because the audio thread counterpart of a plugin is only
        // ever borrowed in this method. Also, the verifier has verified that no
        // data races exist between parallel audio threads (once we actually have
        // multi-threaded schedules of course).
        let plugin_audio_thread = unsafe { &mut *plugin.shared.plugin.get() };

        // TODO: input event stuff

        let status =
            if let Err(_) = plugin_audio_thread.start_processing(&plugin.shared.host_request) {
                ProcessStatus::Error
            } else {
                let status = plugin_audio_thread.process(
                    proc_info,
                    audio_in,
                    audio_out,
                    &plugin.shared.host_request,
                );

                plugin_audio_thread.stop_processing(&plugin.shared.host_request);

                status
            };

        // TODO: output event stuff

        if let ProcessStatus::Error = status {
            // As per the spec, we must clear all output buffers.
            for b in audio_out.iter_mut() {
                b.clear(proc_info);
            }
        }

        // TODO: Other process status stuff
    }
}

pub(crate) struct DelayCompTask {
    pub delay_comp_node: SharedDelayCompNode,

    pub audio_in: SharedAudioBuffer<f32>,
    pub audio_out: SharedAudioBuffer<f32>,
}

impl DelayCompTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        // This is safe because this is only ever borrowed in this method.
        // Also, the verifier has verified that no data races exist between parallel
        // audio threads (once we actually have multi-threaded schedules of course).
        let delay_comp_node = unsafe { &mut *self.delay_comp_node.shared.get() };

        delay_comp_node.process(proc_info, &self.audio_in, &self.audio_out);
    }
}

pub(crate) struct SumTask {
    pub audio_in: SmallVec<[SharedAudioBuffer<f32>; 4]>,
    pub audio_out: SharedAudioBuffer<f32>,
}

impl SumTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        // Please refer to the "SAFETY NOTE" at the top of the file
        // `src/graph/audio_buffer_pool.rs` on why it is considered safe to
        // borrow these buffers.
        //
        // Also the unchecked indexing is safe because all buffers have a length
        // greater than or equal to `proc_info.frames`. The host will never set
        // `proc_info.frames` to a length greater than the allocated size for
        // the audio buffers.
        unsafe {
            let out = self.audio_out.borrow_mut(proc_info);

            // Unroll loops for common number of inputs.
            match self.audio_in.len() {
                0 => return,
                1 => {
                    let in_0 = self.audio_in[0].borrow(proc_info);
                    out.copy_from_slice(&in_0);
                }
                2 => {
                    let in_0 = self.audio_in[0].borrow(proc_info);
                    let in_1 = self.audio_in[1].borrow(proc_info);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i) + *in_1.get_unchecked(i);
                    }
                }
                3 => {
                    let in_0 = self.audio_in[0].borrow(proc_info);
                    let in_1 = self.audio_in[1].borrow(proc_info);
                    let in_2 = self.audio_in[2].borrow(proc_info);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i);
                    }
                }
                4 => {
                    let in_0 = self.audio_in[0].borrow(proc_info);
                    let in_1 = self.audio_in[1].borrow(proc_info);
                    let in_2 = self.audio_in[2].borrow(proc_info);
                    let in_3 = self.audio_in[3].borrow(proc_info);

                    for i in 0..proc_info.frames {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i)
                            + *in_3.get_unchecked(i);
                    }
                }
                num_inputs => {
                    let in_0 = self.audio_in[0].borrow(proc_info);

                    out.copy_from_slice(in_0);

                    for ch_i in 1..num_inputs {
                        let input = self.audio_in[ch_i].borrow(proc_info);
                        for smp_i in 0..proc_info.frames {
                            *out.get_unchecked_mut(smp_i) += *input.get_unchecked(smp_i);
                        }
                    }
                }
            }
        }
    }
}

pub(crate) struct DeactivatedPluginTask {
    pub audio_through: SmallVec<[(SharedAudioBuffer<f32>, SharedAudioBuffer<f32>); 4]>,
    pub extra_audio_out: SmallVec<[SharedAudioBuffer<f32>; 4]>,
}

impl DeactivatedPluginTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        // Please refer to the "SAFETY NOTE" at the top of the file
        // `src/graph/audio_buffer_pool.rs` on why it is considered safe to
        // borrow these buffers.
        //
        // In addition the host will never set `proc_info.frames` to something
        // higher than the maximum frame size (which is what the Vec's initial
        // capacity is set to).
        unsafe {
            // Pass audio through the main ports.
            for (in_buf, out_buf) in self.audio_through.iter() {
                let in_buf = in_buf.borrow(proc_info);
                let out_buf = out_buf.borrow_mut(proc_info);
                out_buf.copy_from_slice(in_buf);
            }

            // Make sure any extra output buffers are cleared.
            for b in self.extra_audio_out.iter() {
                let b = b.borrow_mut(proc_info);
                b.fill(0.0);
            }
        }
    }
}
