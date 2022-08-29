use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use basedrop::Shared;
use dropseed_plugin_api::buffer::SharedBuffer;
use dropseed_plugin_api::ProcInfo;

use crate::plugin_host::event_io_buffers::NoteIoEvent;

pub(crate) struct NoteDelayCompTask {
    pub shared_node: SharedNoteDelayCompNode,

    pub note_in: SharedBuffer<NoteIoEvent>,
    pub note_out: SharedBuffer<NoteIoEvent>,
}

impl NoteDelayCompTask {
    pub fn process(&mut self, proc_info: &ProcInfo) {
        let mut delay_comp_node = self.shared_node.borrow_mut();

        delay_comp_node.process(proc_info, &self.note_in, &self.note_out);
    }
}

#[derive(Clone)]
pub(crate) struct SharedNoteDelayCompNode {
    pub active: bool,
    pub delay: u32,

    shared: Shared<AtomicRefCell<NoteDelayCompNode>>,
}

impl SharedNoteDelayCompNode {
    pub fn new(d: NoteDelayCompNode, coll_handle: &basedrop::Handle) -> Self {
        Self {
            active: true,
            delay: d.delay(),
            shared: Shared::new(coll_handle, AtomicRefCell::new(d)),
        }
    }

    pub fn borrow_mut<'a>(&'a self) -> AtomicRefMut<'a, NoteDelayCompNode> {
        self.shared.borrow_mut()
    }
}

pub(crate) struct NoteDelayCompNode {
    // TODO
    delay: u32,
}

impl NoteDelayCompNode {
    pub fn new(delay: u32) -> Self {
        Self { delay }
    }

    pub fn process(
        &mut self,
        proc_info: &ProcInfo,
        input: &SharedBuffer<NoteIoEvent>,
        output: &SharedBuffer<NoteIoEvent>,
    ) {
        // TODO
    }

    pub fn delay(&self) -> u32 {
        self.delay
    }
}
