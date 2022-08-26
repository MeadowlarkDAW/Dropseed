use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use audio_graph::Edge;
use basedrop::Shared;
use fnv::FnvHashMap;

use crate::processor_schedule::tasks::{
    AudioDelayCompNode, NoteDelayCompNode, ParamEventDelayCompNode,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DelayCompKey {
    pub edge: Edge,
    pub delay: u32,
}

#[derive(Clone)]
pub(crate) struct SharedAudioDelayCompNode {
    pub active: bool,
    pub delay: u32,

    shared: Shared<AtomicRefCell<AudioDelayCompNode>>,
}

impl SharedAudioDelayCompNode {
    pub fn new(d: AudioDelayCompNode, coll_handle: &basedrop::Handle) -> Self {
        Self {
            active: true,
            delay: d.delay(),
            shared: Shared::new(coll_handle, AtomicRefCell::new(d)),
        }
    }

    pub fn borrow_mut<'a>(&'a self) -> AtomicRefMut<'a, AudioDelayCompNode> {
        self.shared.borrow_mut()
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

#[derive(Clone)]
pub(crate) struct SharedParamEventDelayCompNode {
    pub active: bool,
    pub delay: u32,

    shared: Shared<AtomicRefCell<ParamEventDelayCompNode>>,
}

impl SharedParamEventDelayCompNode {
    pub fn new(d: ParamEventDelayCompNode, coll_handle: &basedrop::Handle) -> Self {
        Self {
            active: true,
            delay: d.delay(),
            shared: Shared::new(coll_handle, AtomicRefCell::new(d)),
        }
    }

    pub fn borrow_mut<'a>(&'a self) -> AtomicRefMut<'a, ParamEventDelayCompNode> {
        self.shared.borrow_mut()
    }
}

pub(crate) struct DelayCompNodePool {
    pub audio: FnvHashMap<DelayCompKey, SharedAudioDelayCompNode>,
    pub note: FnvHashMap<DelayCompKey, SharedNoteDelayCompNode>,
    pub param_event: FnvHashMap<DelayCompKey, SharedParamEventDelayCompNode>,
}

impl DelayCompNodePool {
    pub fn new() -> Self {
        Self {
            audio: FnvHashMap::default(),
            note: FnvHashMap::default(),
            param_event: FnvHashMap::default(),
        }
    }
}
