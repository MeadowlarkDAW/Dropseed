use smallvec::SmallVec;

use dropseed_plugin_api::automation::AutomationIoEvent;
use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::plugin_host::event_io_buffers::NoteIoEvent;

pub(crate) struct UnloadedPluginTask {
    pub audio_through: SmallVec<[(SharedBuffer<f32>, SharedBuffer<f32>); 4]>,
    pub note_through: Option<(SharedBuffer<NoteIoEvent>, SharedBuffer<NoteIoEvent>)>,

    pub clear_audio_out: SmallVec<[SharedBuffer<f32>; 4]>,
    pub clear_note_out: SmallVec<[SharedBuffer<NoteIoEvent>; 2]>,
    pub clear_automation_out: Option<SharedBuffer<AutomationIoEvent>>,
}

impl UnloadedPluginTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // Pass audio through the main ports.
        for (in_buf, out_buf) in self.audio_through.iter() {
            out_buf.set_constant(in_buf.is_constant());

            let in_buf_ref = in_buf.borrow();
            let mut out_buf_ref = out_buf.borrow_mut();

            let in_buf_part = &in_buf_ref[0..proc_info.frames];
            let out_buf_part = &mut out_buf_ref[0..proc_info.frames];

            out_buf_part.copy_from_slice(in_buf_part);
        }

        // Pass notes through the main ports.
        if let Some((in_buf, out_buf)) = &self.note_through {
            let in_buf_ref = in_buf.borrow();
            let mut out_buf_ref = out_buf.borrow_mut();

            out_buf_ref.clear();
            out_buf_ref.clone_from(&*in_buf_ref);
        }

        // Make sure all output buffers are cleared.
        for out_buf in self.clear_audio_out.iter() {
            out_buf.clear(proc_info.frames);
            out_buf.set_constant(true);
        }
        for out_buf in self.clear_note_out.iter() {
            out_buf.truncate();
        }
        if let Some(out_buf) = &self.clear_automation_out {
            out_buf.truncate();
        }
    }
}
