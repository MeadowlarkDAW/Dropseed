mod audio_thread;
mod main_thread;

pub use audio_thread::DsHostAudioThr;
pub use main_thread::DsHostMainThr;

pub fn new_plugin_host() -> (DsHostMainThr, DsHostAudioThr) {
    (DsHostMainThr::new(), DsHostAudioThr::new())
}
