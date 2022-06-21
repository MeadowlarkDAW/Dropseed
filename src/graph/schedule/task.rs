use smallvec::SmallVec;

use crate::graph::shared_pool::{SharedBuffer, SharedDelayCompNode, SharedPluginHostAudioThread};
use crate::plugin::process_info::ProcBuffers;
use crate::{ProcEvent, ProcInfo};

use super::sum::SumTask;

pub(crate) enum Task {
    Plugin(PluginTask),
    DelayComp(DelayCompTask),
    Sum(SumTask),
    DeactivatedPlugin(DeactivatedPluginTask),
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Task::Plugin(t) => {
                let mut f = f.debug_struct("Plugin");

                f.field("id", &t.plugin.id());

                if !t.buffers.audio_in.is_empty() {
                    let mut s = String::new();
                    for b in t.buffers.audio_in.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_in", &s);
                }

                if !t.buffers.audio_out.is_empty() {
                    let mut s = String::new();
                    for b in t.buffers.audio_out.iter() {
                        s.push_str(&format!("{:?}, ", b))
                    }

                    f.field("audio_out", &s);
                }

                if let Some(automation_in_buffers) = &t.automation_in_buffers {
                    let mut s = String::new();
                    for b in automation_in_buffers.iter() {
                        s.push_str(&format!("{:?}, ", b.id()))
                    }

                    f.field("automation_in", &s);
                }

                if let Some(automation_out_buffer) = &t.automation_out_buffer {
                    f.field("automation_out", &format!("{:?}", automation_out_buffer.id()));
                }

                if !t.note_in_buffers.is_empty() {
                    let mut has_buffer = false;
                    let mut s = String::new();
                    for buffers in t.note_in_buffers.iter() {
                        if let Some(buffers) = buffers {
                            has_buffer = true;

                            s.push_str("[");

                            for b in buffers.iter() {
                                s.push_str(&format!("{:?}, ", b.id()))
                            }

                            s.push_str("], ");
                        }
                    }

                    if has_buffer {
                        f.field("note_in", &s);
                    }
                }

                if !t.note_out_buffers.is_empty() {
                    let mut has_buffer = false;
                    let mut s = String::new();
                    for buffer in t.note_out_buffers.iter() {
                        if let Some(b) = buffer {
                            has_buffer = true;

                            s.push_str(&format!("{:?}, ", b.id()));
                        }
                    }

                    if has_buffer {
                        f.field("note_out", &s);
                    }
                }

                f.finish()
            }
            Task::DelayComp(t) => {
                let mut f = f.debug_struct("DelayComp");

                f.field("audio_in", t.audio_in.id());
                f.field("audio_out", t.audio_out.id());
                f.field("delay", &t.delay_comp_node.delay());

                f.finish()
            }
            Task::Sum(t) => {
                let mut f = f.debug_struct("Sum");

                let mut s = String::new();
                for b in t.audio_in.iter() {
                    s.push_str(&format!("{:?}, ", b.id()))
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
                    for buffer in t.note_out_buffers.iter() {
                        if let Some(b) = buffer {
                            has_buffer = true;

                            s.push_str(&format!("{:?}, ", b.id()));
                        }
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

pub(crate) struct PluginTask {
    pub plugin: SharedPluginHostAudioThread,

    pub buffers: ProcBuffers,

    pub automation_in_buffers: Option<SmallVec<[SharedBuffer<ProcEvent>; 2]>>,
    pub automation_out_buffer: Option<SharedBuffer<ProcEvent>>,

    pub note_in_buffers: SmallVec<[Option<SmallVec<[SharedBuffer<ProcEvent>; 2]>>; 2]>,
    pub note_out_buffers: SmallVec<[Option<SharedBuffer<ProcEvent>>; 2]>,
}

impl PluginTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        // SAFETY
        // - This is only ever borrowed here in this method in the audio thread.
        // - The schedule verifier has ensured that a single plugin instance does
        // not appear twice within the same schedule, so no data races can occur.
        let mut plugin_audio_thread = unsafe { (*self.plugin.plugin).borrow_mut() };

        plugin_audio_thread.process(
            proc_info,
            &mut self.buffers,
            &self.automation_in_buffers,
            &self.automation_out_buffer,
            &self.note_in_buffers,
            &self.note_out_buffers,
        );
    }
}

pub(crate) struct DelayCompTask {
    pub delay_comp_node: SharedDelayCompNode,

    pub audio_in: SharedBuffer<f32>,
    pub audio_out: SharedBuffer<f32>,
}

impl DelayCompTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        // SAFETY
        // - This is only ever borrowed here in this method in the audio thread.
        // - The schedule verifier has ensured that a single node instance does
        // not appear twice within the same schedule, so no data races can occur.
        let mut delay_comp_node = unsafe { (*self.delay_comp_node.node).borrow_mut() };

        delay_comp_node.process(proc_info, &self.audio_in, &self.audio_out);
    }
}

pub(crate) struct DeactivatedPluginTask {
    pub audio_through: SmallVec<[(SharedBuffer<f32>, SharedBuffer<f32>); 4]>,
    pub extra_audio_out: SmallVec<[SharedBuffer<f32>; 4]>,

    pub automation_out_buffer: Option<SharedBuffer<ProcEvent>>,

    pub note_out_buffers: SmallVec<[Option<SharedBuffer<ProcEvent>>; 2]>,
}

impl DeactivatedPluginTask {
    fn process(&mut self, proc_info: &ProcInfo) {
        // SAFETY
        // - These buffers are only ever borrowed in the audio thread.
        // - The schedule verifier has ensured that no data races can occur between parallel
        // audio threads due to aliasing buffer pointers.
        // - `proc_info.frames` will always be less than or equal to the allocated size of
        // all process audio buffers.
        unsafe {
            // Pass audio through the main ports.
            for (in_buf, out_buf) in self.audio_through.iter() {
                out_buf.set_constant(in_buf.is_constant());

                let in_buf_ref = in_buf.borrow();
                let mut out_buf_ref = out_buf.borrow_mut();

                #[cfg(debug_assertions)]
                let in_buf = &in_buf_ref[0..proc_info.frames];
                #[cfg(debug_assertions)]
                let out_buf = &mut out_buf_ref[0..proc_info.frames];

                #[cfg(not(debug_assertions))]
                let in_buf = std::slice::from_raw_parts(in_buf_ref.as_ptr(), proc_info.frames);
                #[cfg(not(debug_assertions))]
                let out_buf =
                    std::slice::from_raw_parts_mut(out_buf_ref.as_mut_ptr(), proc_info.frames);

                out_buf.copy_from_slice(in_buf);
            }

            // Make sure all output buffers are cleared.
            for out_buf in self.extra_audio_out.iter() {
                out_buf.clear_f32(proc_info.frames);
            }
            if let Some(out_buf) = &self.automation_out_buffer {
                let mut b = out_buf.borrow_mut();
                b.clear();
            }
            for out_buf in self.note_out_buffers.iter() {
                if let Some(out_buf) = out_buf {
                    let mut b = out_buf.borrow_mut();
                    b.clear();
                }
            }
        }
    }
}
