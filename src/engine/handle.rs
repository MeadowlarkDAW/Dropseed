use crossbeam::channel::{self, Receiver, Sender};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::engine::events::from_engine::DSEngineEvent;
use crate::engine::events::to_engine::DSEngineRequest;
use crate::engine::main_thread::DSEngineMainThread;
use crate::engine::plugin_scanner::ScannedPluginKey;
use crate::plugin::host_request::HostInfo;
use crate::plugin::PluginFactory;

pub struct DSEngineHandle {
    /// The results of scanning the internal plugins.
    pub internal_plugins_res: Vec<Result<ScannedPluginKey, String>>,

    handle_to_engine_tx: Sender<DSEngineRequest>,

    // TODO: Actually make this a sandboxed thread/process (which is the whole point of using
    // a message passing model in the first place).
    sandboxed_thread_handle: Option<JoinHandle<()>>,
    run_sandboxed_thread: Arc<AtomicBool>,

    _event_tx: Sender<DSEngineEvent>,

    host_info: HostInfo,
}

struct SpawnEngineRes {
    internal_plugin_res: Vec<Result<ScannedPluginKey, String>>,
}

impl DSEngineHandle {
    pub fn new(
        host_info: HostInfo,
        internal_plugins: Vec<Box<dyn PluginFactory>>,
    ) -> (Self, Receiver<DSEngineEvent>) {
        let (event_tx, event_rx) = channel::unbounded::<DSEngineEvent>();
        let (handle_to_engine_tx, handle_to_engine_rx) = channel::unbounded::<DSEngineRequest>();

        let (spawn_engine_res_tx, spawn_engine_res_rx) = channel::bounded::<SpawnEngineRes>(1);

        let host_info_clone = host_info.clone();

        let run_sandboxed_thread = Arc::new(AtomicBool::new(true));
        let run_sandboxed_thread_clone = Arc::clone(&run_sandboxed_thread);

        let event_tx_clone = event_tx.clone();

        // TODO: Use a sandboxed thread/process (which is the whole point of using a message
        // passing model in the first place).
        let sandboxed_thread_handle = thread::spawn(move || {
            let (mut engine, internal_plugin_res) = DSEngineMainThread::new(
                host_info_clone,
                internal_plugins,
                handle_to_engine_rx,
                event_tx_clone,
            );

            spawn_engine_res_tx.send(SpawnEngineRes { internal_plugin_res }).unwrap();
            let _ = spawn_engine_res_tx;

            engine.run(run_sandboxed_thread);
        });

        let spawn_engine_res = spawn_engine_res_rx.recv_timeout(Duration::from_secs(20)).unwrap();
        let _ = spawn_engine_res_rx;

        (
            Self {
                internal_plugins_res: spawn_engine_res.internal_plugin_res,
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
    pub fn send(&mut self, msg: DSEngineRequest) {
        self.handle_to_engine_tx.send(msg).unwrap();
    }

    pub fn host_info(&self) -> &HostInfo {
        &self.host_info
    }
}

impl Drop for DSEngineHandle {
    fn drop(&mut self) {
        if let Some(sandboxed_thread_handle) = self.sandboxed_thread_handle.take() {
            self.run_sandboxed_thread.store(false, Ordering::Relaxed);
            if let Err(e) = sandboxed_thread_handle.join() {
                log::error!("Failed to join sandboxed thread: {:?}", &e);
            }
        }
    }
}
