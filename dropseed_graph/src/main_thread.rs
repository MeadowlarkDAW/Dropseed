use crate::channel::MainToAudioMsg;

use rtrb::Producer;
use dropseed_core::GCHandle;

pub struct DsGraphMainThr {
    to_audio_tx: Producer<MainToAudioMsg>,
    gc_handle: GCHandle,
}

impl DsGraphMainThr {
    pub(crate) fn new(to_audio_tx: Producer<MainToAudioMsg>, gc_handle: GCHandle) -> Self {
        Self {
            to_audio_tx,
            gc_handle
        }
    }

    /// Returns `true` if the corresponding `DsGraphAudioThr` struct was dropped.
    pub fn did_audio_thread_drop(&self) -> bool {
        self.to_audio_tx.is_abandoned()
    }
}