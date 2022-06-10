use crossbeam::channel::{self, Receiver, Sender};
use std::error::Error;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::engine::events::from_engine::DAWEngineEvent;
use crate::engine::events::to_engine::DAWEngineRequest;
use crate::engine::main_thread::DAWEngineMainThread;
use crate::engine::plugin_scanner::ScannedPluginKey;
use crate::plugin::host_request::HostInfo;
use crate::plugin::PluginFactory;

pub struct DAWEngineHandle {
    /// The results of scanning the internal plugins.
    pub internal_plugins_res: Vec<Result<ScannedPluginKey, Box<dyn Error + Send>>>,

    handle_to_engine_tx: Sender<DAWEngineRequest>,

    // TODO: Actually make this a sandboxed thread/process (which is the whole point of using
    // a message passing model in the first place).
    sandboxed_thread_handle: Option<JoinHandle<()>>,
    run_sandboxed_thread: Arc<AtomicBool>,

    _event_tx: Sender<DAWEngineEvent>,

    host_info: HostInfo,
}

impl DAWEngineHandle {
    pub fn new(
        host_info: HostInfo,
        internal_plugins: Vec<Box<dyn PluginFactory>>,
    ) -> (Self, Receiver<DAWEngineEvent>) {
        let (event_tx, event_rx) = channel::unbounded::<DAWEngineEvent>();
        let (handle_to_engine_tx, handle_to_engine_rx) = channel::unbounded::<DAWEngineRequest>();

        let (internal_plugin_res_tx, internal_plugin_res_rx) =
            channel::bounded::<Vec<Result<ScannedPluginKey, Box<dyn Error + Send>>>>(1);

        let host_info_clone = host_info.clone();

        let run_sandboxed_thread = Arc::new(AtomicBool::new(true));
        let run_sandboxed_thread_clone = Arc::clone(&run_sandboxed_thread);

        let event_tx_clone = event_tx.clone();

        // TODO: Use a sandboxed thread/process (which is the whole point of using a message
        // passing model in the first place).
        let sandboxed_thread_handle = thread::spawn(move || {
            let (mut engine, internal_plugin_res) = DAWEngineMainThread::new(
                host_info_clone,
                internal_plugins,
                handle_to_engine_rx,
                event_tx_clone,
            );

            internal_plugin_res_tx.send(internal_plugin_res).unwrap();
            let _ = internal_plugin_res_tx;

            engine.run(run_sandboxed_thread);
        });

        let internal_plugins_res =
            internal_plugin_res_rx.recv_timeout(Duration::from_secs(20)).unwrap();
        let _ = internal_plugin_res_rx;

        (
            Self {
                internal_plugins_res,
                handle_to_engine_tx,
                sandboxed_thread_handle: Some(sandboxed_thread_handle),
                run_sandboxed_thread: run_sandboxed_thread_clone,
                _event_tx: event_tx,
                host_info,
            },
            event_rx,
        )
    }

    /// Send a request to the engine.
    ///
    /// Note that the engine may decide to ignore invalid requests.
    pub fn send(&mut self, msg: DAWEngineRequest) {
        self.handle_to_engine_tx.send(msg).unwrap();
    }

    pub fn host_info(&self) -> &HostInfo {
        &self.host_info
    }
}

impl Drop for DAWEngineHandle {
    fn drop(&mut self) {
        if let Some(sandboxed_thread_handle) = self.sandboxed_thread_handle.take() {
            self.run_sandboxed_thread.store(false, Ordering::Relaxed);
            if let Err(e) = sandboxed_thread_handle.join() {
                log::error!("Failed to join sandboxed thread: {:?}", &e);
            }
        }
    }
}
