use crate::channel::MainToAudioMsg;

use rtrb::Consumer;

pub struct DsGraphAudioThr {
    from_main_rx: Consumer<MainToAudioMsg>,
}

impl DsGraphAudioThr {
    pub(crate) fn new(from_main_rx: Consumer<MainToAudioMsg>) -> Self {
        Self {
            from_main_rx,
        }
    }

    /// Returns `true` if the corresponding `DsGraphMainThr` struct was dropped.
    pub fn did_main_thread_drop(&self) -> bool {
        self.from_main_rx.is_abandoned()
    }
}