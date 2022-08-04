use smallvec::SmallVec;

use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::plugin_host::events::{NoteEvent, ParamEvent};

pub(crate) struct DeactivatedPluginTask {
    pub audio_through: SmallVec<[(SharedBuffer<f32>, SharedBuffer<f32>); 4]>,
    pub extra_audio_out: SmallVec<[SharedBuffer<f32>; 4]>,

    pub automation_out_buffer: Option<SharedBuffer<ParamEvent>>,

    pub note_out_buffers: SmallVec<[Option<SharedBuffer<NoteEvent>>; 2]>,
}

impl DeactivatedPluginTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // Pass audio through the main ports.
        for (in_buf, out_buf) in self.audio_through.iter() {
            out_buf.set_constant(in_buf.is_constant());

            let in_buf_ref = in_buf.borrow();
            let mut out_buf_ref = out_buf.borrow_mut();

            let in_buf = &in_buf_ref[0..proc_info.frames];
            let out_buf = &mut out_buf_ref[0..proc_info.frames];

            out_buf.copy_from_slice(in_buf);
        }

        // Make sure all output buffers are cleared.
        for out_buf in self.extra_audio_out.iter() {
            out_buf.clear_until(proc_info.frames);
        }
        if let Some(out_buf) = &self.automation_out_buffer {
            out_buf.truncate();
        }
        for out_buf in self.note_out_buffers.iter().flatten() {
            out_buf.truncate();
        }
    }
}
