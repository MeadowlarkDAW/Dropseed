use std::borrow::Borrow;

use basedrop::Shared;
use clap_sys::process::clap_process;
use smallvec::SmallVec;

use crate::graph::audio_buffer_pool::SharedAudioBuffer;
use crate::graph::plugin_pool::{SharedDelayCompNode, SharedPluginAudioThreadInstance};
use crate::{host::Host, AudioPortBuffer, ProcInfo, ProcessStatus};

use super::delay_comp_node::DelayCompNode;

pub(crate) enum Task {
    InternalPlugin(InternalPluginTask),
    ClapPlugin(ClapPluginTask),
    DelayComp(DelayCompTask),
    Sum(SumTask),
    InactivePlugin(InactivePluginTask),
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
            Task::InactivePlugin(t) => {
                let mut f = f.debug_struct("InactivePlugin");

                let mut s = String::new();
                for b in t.audio_out.iter() {
                    s.push_str(&format!("{:?}, ", b.unique_id()))
                }

                f.field("audio_out", &s);

                f.finish()
            }
        }
    }
}

impl Task {
    pub fn process(&mut self, info: &ProcInfo, host: &mut Host) {
        match self {
            Task::InternalPlugin(task) => {
                let status = task.process(info, host);

                // TODO: use process status
            }
            Task::ClapPlugin(task) => {
                todo!()
            }
            Task::DelayComp(task) => task.process(info),
            Task::Sum(task) => task.process(info),
            Task::InactivePlugin(task) => task.process(info, host),
        }
    }
}

pub(crate) struct InternalPluginTask {
    pub plugin: SharedPluginAudioThreadInstance,

    pub audio_in: SmallVec<[AudioPortBuffer; 2]>,
    pub audio_out: SmallVec<[AudioPortBuffer; 2]>,
}

impl InternalPluginTask {
    fn process(&mut self, info: &ProcInfo, host: &mut Host) -> ProcessStatus {
        let Self { plugin, audio_in, audio_out } = self;

        // This is safe because the audio thread counterpart of a plugin is only ever
        // borrowed mutably in this method. Also, the verifier has verified that no
        // data races exist between parallel audio threads (once we actually have
        // multi-threaded schedules of course).
        let plugin = unsafe { plugin.borrow_mut() };

        // Prepare the host handle to accept requests from the plugin.
        host.current_plugin_channel = Shared::clone(&plugin.channel);

        // TODO: input event stuff

        let status = if let Err(_) = plugin.plugin.start_processing(host) {
            ProcessStatus::Error
        } else {
            let status = plugin.plugin.process(info, audio_in, audio_out, host);

            plugin.plugin.stop_processing(host);

            status
        };

        if let ProcessStatus::Error = status {
            // As per the spec, we must clear all output buffers.
            for b in audio_out.iter_mut() {
                b.clear(info);
            }
        }

        // TODO: output event stuff

        status
    }
}

pub(crate) struct DelayCompTask {
    pub delay_comp_node: SharedDelayCompNode,

    pub audio_in: SharedAudioBuffer<f32>,
    pub audio_out: SharedAudioBuffer<f32>,
}

impl DelayCompTask {
    fn process(&mut self, info: &ProcInfo) {
        // This is safe because this is only ever borrowed mutably in this method.
        // Also, the verifier has verified that no data races exist between parallel
        // audio threads (once we actually have multi-threaded schedules of course).
        let delay_comp_node = unsafe { &mut *self.delay_comp_node.shared.get() };

        delay_comp_node.process(info, &self.audio_in, &self.audio_out);
    }
}

pub(crate) struct SumTask {
    pub audio_in: SmallVec<[SharedAudioBuffer<f32>; 4]>,
    pub audio_out: SharedAudioBuffer<f32>,
}

impl SumTask {
    fn process(&mut self, info: &ProcInfo) {
        let out = self.audio_out.borrow_mut(info);

        // Unroll loops for common number of inputs.
        match self.audio_in.len() {
            0 => return,
            1 => {
                let in_0 = self.audio_in[0].borrow(info);
                out.copy_from_slice(&in_0);
            }
            2 => {
                let in_0 = self.audio_in[0].borrow(info);
                let in_1 = self.audio_in[1].borrow(info);

                for i in 0..info.frames {
                    unsafe {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i) + *in_1.get_unchecked(i);
                    }
                }
            }
            3 => {
                let in_0 = self.audio_in[0].borrow(info);
                let in_1 = self.audio_in[1].borrow(info);
                let in_2 = self.audio_in[2].borrow(info);

                for i in 0..info.frames {
                    unsafe {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i);
                    }
                }
            }
            4 => {
                let in_0 = self.audio_in[0].borrow(info);
                let in_1 = self.audio_in[1].borrow(info);
                let in_2 = self.audio_in[2].borrow(info);
                let in_3 = self.audio_in[3].borrow(info);

                for i in 0..info.frames {
                    unsafe {
                        *out.get_unchecked_mut(i) = *in_0.get_unchecked(i)
                            + *in_1.get_unchecked(i)
                            + *in_2.get_unchecked(i)
                            + *in_3.get_unchecked(i);
                    }
                }
            }
            num_inputs => {
                let in_0 = self.audio_in[0].borrow(info);

                out.copy_from_slice(in_0);

                for i in 1..num_inputs {
                    let input = self.audio_in[i].borrow(info);
                    unsafe {
                        *out.get_unchecked_mut(i) += *input.get_unchecked(i);
                    }
                }
            }
        }
    }
}

pub(crate) struct InactivePluginTask {
    pub audio_out: SmallVec<[SharedAudioBuffer<f32>; 4]>,
}

impl InactivePluginTask {
    fn process(&mut self, info: &ProcInfo, host: &mut Host) {
        // Make sure output buffers are cleared.
        for b in self.audio_out.iter() {
            let b = b.borrow_mut(info);
            b.fill(0.0);
        }
    }
}

pub(crate) struct ClapPluginTask {
    // TODO: clap processor
//ports: ClapProcAudioPorts,
}

impl ClapPluginTask {
    fn process(&mut self, proc: &mut clap_process) -> ProcessStatus {
        // Prepare the buffers to be sent to the external plugin.
        //self.ports.prepare(proc);

        // TODO: process clap plugin

        todo!()
    }
}
