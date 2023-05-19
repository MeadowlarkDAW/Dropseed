mod process_thread;
mod main_thread;
mod settings;
pub(crate) mod channel;

use rtrb::RingBuffer;
use dropseed_core::GCHandle;

pub use process_thread::DsGraphAudioThr;
pub use main_thread::DsGraphMainThr;
pub use settings::DsGraphSettings;

/// Create a new Dropseed graph.
/// 
/// This will return the main thread counterpart and the audio thread
/// counterpart of the graph. Send the `DsGraphAudioThr` counterpart to
/// your realtime audio thread.
pub fn new_dropseed_graph(settings: &DsGraphSettings, gc_handle: GCHandle) -> (DsGraphMainThr, DsGraphAudioThr) {
    let (to_audio_tx, from_main_rx) = RingBuffer::new(settings.channel_size.max(16));

    (DsGraphMainThr::new(to_audio_tx, gc_handle), DsGraphAudioThr::new(from_main_rx))
}