use atomic_refcell::{AtomicRef, AtomicRefCell};
use basedrop::Shared;
use meadowlark_core_types::time::{FrameTime, SecondsF64};

pub static DEFAULT_DECLICK_TIME: SecondsF64 = SecondsF64(3.0 / 1000.0);

pub struct DeclickBuffers {
    pub start_stop_buf: Vec<f32>,
    pub jump_out_buf: Vec<f32>,
    pub jump_in_buf: Vec<f32>,
}

#[derive(Clone)]
pub struct DeclickInfo {
    // TODO: Explain what each of these fields mean.
    buffers: Shared<AtomicRefCell<DeclickBuffers>>,

    pub start_stop_active: bool,
    pub jump_active: bool,

    pub jump_in_playhead: i64,
    pub jump_out_playhead: FrameTime,

    pub start_declick_start: FrameTime,
    pub jump_in_declick_start: i64,
}

impl DeclickInfo {
    pub fn _new(
        buffers: Shared<AtomicRefCell<DeclickBuffers>>,
        start_stop_active: bool,
        jump_active: bool,
        jump_in_playhead: i64,
        jump_out_playhead: FrameTime,
        start_declick_start: FrameTime,
        jump_in_declick_start: i64,
    ) -> Self {
        Self {
            buffers,
            start_stop_active,
            jump_active,
            jump_in_playhead,
            jump_out_playhead,
            start_declick_start,
            jump_in_declick_start,
        }
    }

    pub fn buffers(&self) -> AtomicRef<'_, DeclickBuffers> {
        self.buffers.borrow()
    }
}
