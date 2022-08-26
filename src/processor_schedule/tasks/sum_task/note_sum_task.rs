use smallvec::SmallVec;

use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::plugin_host::event_io_buffers::NoteIoEvent;

pub(crate) struct NoteSumTask {
    pub note_in: SmallVec<[SharedBuffer<NoteIoEvent>; 4]>,
    pub note_out: SharedBuffer<NoteIoEvent>,
}

impl NoteSumTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        // TODO
    }
}
