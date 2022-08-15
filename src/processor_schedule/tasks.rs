use dropseed_plugin_api::ProcInfo;
use std::fmt::{Debug, Error, Formatter, Write};

mod deactivated_plug_task;
mod delay_comp_task;
mod plugin_task;
mod sum_task;
mod transport_task;

pub use transport_task::TransportHandle;

pub(crate) use deactivated_plug_task::DeactivatedPluginTask;
pub(crate) use delay_comp_task::{DelayCompNode, DelayCompTask};
pub(crate) use plugin_task::PluginTask;
pub(crate) use sum_task::SumTask;
pub(crate) use transport_task::TransportTask;

pub(crate) enum Task {
    Plugin(PluginTask),
    DelayComp(DelayCompTask),
    Sum(SumTask),
    DeactivatedPlugin(DeactivatedPluginTask),
}

impl Debug for Task {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        // TODO: Move the debug printing for enum variants into the respective modules.
        match self {
            Task::Plugin(t) => {
                let mut f = f.debug_struct("Plugin");

                f.field("id", &t.plugin_id);

                if !t.buffers.audio_in.is_empty() {
                    let mut s = String::new();
                    for b in t.buffers.audio_in.iter() {
                        write!(s, "{:?}, ", b)?;
                    }

                    f.field("audio_in", &s);
                }

                if !t.buffers.audio_out.is_empty() {
                    let mut s = String::new();
                    for b in t.buffers.audio_out.iter() {
                        write!(s, "{:?}, ", b)?;
                    }

                    f.field("audio_out", &s);
                }

                if let Some(automation_in_buffers) = &t.event_buffers.unmixed_param_in_buffers {
                    let mut s = String::new();
                    for b in automation_in_buffers.iter() {
                        s.push_str(&format!("{:?}, ", b.id()))
                    }

                    f.field("automation_in", &s);
                }

                if let Some(automation_out_buffer) = &t.event_buffers.param_out_buffer {
                    f.field("automation_out", &format!("{:?}", automation_out_buffer.id()));
                }

                if !t.event_buffers.unmixed_note_in_buffers.is_empty() {
                    let mut has_buffer = false;
                    let mut s = String::new();
                    for buffers in t.event_buffers.unmixed_note_in_buffers.iter().flatten() {
                        has_buffer = true;

                        s.push('[');

                        for b in buffers.iter() {
                            s.push_str(&format!("{:?}, ", b.id()))
                        }

                        s.push_str("], ");
                    }

                    if has_buffer {
                        f.field("note_in", &s);
                    }
                }

                if !t.event_buffers.note_out_buffers.is_empty() {
                    let mut has_buffer = false;
                    let mut s = String::new();
                    for buffer in t.event_buffers.note_out_buffers.iter().flatten() {
                        has_buffer = true;

                        s.push_str(&format!("{:?}, ", buffer.id()));
                    }

                    if has_buffer {
                        f.field("note_out", &s);
                    }
                }

                f.finish()
            }
            Task::DelayComp(t) => {
                let mut f = f.debug_struct("DelayComp");

                f.field("audio_in", &t.audio_in.id());
                f.field("audio_out", &t.audio_out.id());
                f.field("delay", &t.shared_node.delay);

                f.finish()
            }
            Task::Sum(t) => {
                let mut f = f.debug_struct("Sum");

                let mut s = String::new();
                for b in t.audio_in.iter() {
                    write!(s, "{:?}, ", b.id())?;
                }
                f.field("audio_in", &s);

                f.field("audio_out", &format!("{:?}", t.audio_out.id()));

                f.finish()
            }
            Task::DeactivatedPlugin(t) => {
                let mut f = f.debug_struct("DeactivatedPlugin");

                let mut s = String::new();
                for (b_in, b_out) in t.audio_through.iter() {
                    s.push_str(&format!("(in: {:?}, out: {:?})", b_in.id(), b_out.id()));
                }
                f.field("audio_through", &s);

                let mut s = String::new();
                for b in t.extra_audio_out.iter() {
                    s.push_str(&format!("{:?}, ", b.id()))
                }
                f.field("extra_audio_out", &s);

                if let Some(automation_out_buffer) = &t.automation_out_buffer {
                    f.field("automation_out", &format!("{:?}", automation_out_buffer.id()));
                }

                if !t.note_out_buffers.is_empty() {
                    let mut has_buffer = false;
                    let mut s = String::new();
                    for buffer in t.note_out_buffers.iter().flatten() {
                        has_buffer = true;

                        s.push_str(&format!("{:?}, ", buffer.id()));
                    }

                    if has_buffer {
                        f.field("note_out", &s);
                    }
                }

                f.finish()
            }
        }
    }
}

impl Task {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        match self {
            Task::Plugin(task) => task.process(proc_info),
            Task::DelayComp(task) => task.process(proc_info),
            Task::Sum(task) => task.process(proc_info),
            Task::DeactivatedPlugin(task) => task.process(proc_info),
        }
    }
}
