use crate::channel::MainToAudioMsg;

use dropseed_core::RtGCHandle;
use rtrb::Producer;

pub struct DsGraphMainThr {
    to_audio_tx: Producer<MainToAudioMsg>,
    gc: RtGCHandle,
}

impl DsGraphMainThr {
    pub(crate) fn new(to_audio_tx: Producer<MainToAudioMsg>, gc: RtGCHandle) -> Self {
        Self { to_audio_tx, gc }
    }

    /// Returns `true` if the corresponding `DsGraphAudioThr` struct was dropped.
    pub fn did_audio_thread_drop(&self) -> bool {
        self.to_audio_tx.is_abandoned()
    }
}
