use std::cell::UnsafeCell;
use std::marker::PhantomPinned;
use std::os::raw::{c_char, c_void};
use std::path::PathBuf;
use std::ptr::{self, NonNull};
use std::sync::Arc;

use basedrop::Shared;
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::clap_process;
use clap_sys::version::CLAP_VERSION;
use raw_window_handle::RawWindowHandle;
use rusty_daw_core::SampleRate;
use smallvec::SmallVec;

use crate::audio_buffer::ClapAudioBuffer;
use crate::engine::RustyDAWEngine;
use crate::error::{ClapPluginActivationError, ClapPluginThreadError};
use crate::info::HostInfo;
use crate::process::{ClapAudioPorts, ProcessStatus};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginState {
    // The plugin is inactive, only the main thread uses it
    Inactive,

    // Activation failed
    InactiveWithError,

    // The plugin is active and sleeping, the audio engine can call set_processing()
    ActiveAndSleeping,

    // The plugin is processing
    ActiveAndProcessing,

    // The plugin did process but is in error
    ActiveWithError,

    // The plugin is not used anymore by the audio engine and can be deactivated on the main
    // thread
    ActiveAndReadyToDeactivate,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreadState {
    Unknown,
    MainThread,
    AudioThread,
    AudioThreadPool,
}

struct ClapPluginInstanceShared {
    shared: Shared<UnsafeCell<ClapPluginInstance>>,
}

impl Clone for ClapPluginInstanceShared {
    fn clone(&self) -> Self {
        Self {
            shared: Shared::clone(&self.shared),
        }
    }
}

impl ClapPluginInstanceShared {
    pub fn new(info: Arc<HostInfo>, coll_handle: &basedrop::Handle) -> Self {
        let mut raw_clap_host = clap_host {
            clap_version: CLAP_VERSION,
            host_data: ptr::null_mut(),
            name: ptr::null_mut(),
            vendor: ptr::null_mut(),
            url: ptr::null_mut(),
            version: ptr::null_mut(),
            get_extension,
            request_restart,
            request_process,
            request_callback,
        };

        // This is safe because the lifetime of the `HostInfo` is forced
        // to be at-least be the lifetime of this `PluginHost` struct via
        // the `Arc<HostInfo>` pointer being stored in the struct itself.
        unsafe {
            info.write_to_raw(&mut raw_clap_host);
        }

        let shared = Shared::new(
            coll_handle,
            UnsafeCell::new(ClapPluginInstance {
                raw_clap_host,
                info,
                is_loaded: false,
                is_gui_created: false,
                is_gui_visible: false,
                schedule_process: false,
                schedule_restart: false,
                schedule_deactivate: false,
                state: PluginState::Inactive,
                raw_plugin: None,
                audio_ports: ClapAudioPorts::new(SmallVec::new(), SmallVec::new()),
                thread_state: ThreadState::MainThread,
                _phantom_pinned: PhantomPinned::default(),
            }),
        );

        // What a convoluted way to get an `*mut c_void` pointer :/
        let raw_ptr = &(*shared) as *const _ as *const c_void as *mut c_void;

        // Safe because we still own the only reference to this Shared pointer.
        //
        // Also this is safe in the long term because the `Shared` smart pointer
        // never moves its contents.
        unsafe {
            (&mut *(*shared).get()).raw_clap_host.host_data = raw_ptr;
        }

        Self { shared }
    }

    pub fn as_main_thread<'a>(
        &'a mut self,
    ) -> Result<ClapPluginMainThread<'a>, ClapPluginThreadError> {
        let plugin = self.borrow_mut();
        plugin.check_for_main_thread()?;

        Ok(ClapPluginMainThread { plugin })
    }

    pub fn as_audio_thread<'a>(
        &'a mut self,
    ) -> Result<ClapPluginAudioThread<'a>, ClapPluginThreadError> {
        let plugin = self.borrow_mut();
        plugin.check_for_audio_thread()?;

        Ok(ClapPluginAudioThread { plugin })
    }

    fn borrow(&self) -> &ClapPluginInstance {
        unsafe { &*(self.shared.get()) }
    }

    fn borrow_mut(&mut self) -> &mut ClapPluginInstance {
        unsafe { &mut *(self.shared.get()) }
    }
}

struct ClapPluginInstance {
    raw_clap_host: clap_host,

    info: Arc<HostInfo>,

    is_loaded: bool,
    is_gui_created: bool,
    is_gui_visible: bool,
    schedule_process: bool,
    schedule_restart: bool,
    schedule_deactivate: bool,

    state: PluginState,

    raw_plugin: Option<NonNull<clap_plugin>>,

    audio_ports: ClapAudioPorts,

    thread_state: ThreadState,

    _phantom_pinned: PhantomPinned,
}

impl ClapPluginInstance {
    fn is_main_thread(&self) -> bool {
        self.thread_state == ThreadState::MainThread
    }

    fn is_audio_thread(&self) -> bool {
        self.thread_state == ThreadState::AudioThread
    }

    fn check_for_main_thread(&self) -> Result<(), ClapPluginThreadError> {
        if self.thread_state != ThreadState::MainThread {
            Err(ClapPluginThreadError {
                requested_state: ThreadState::MainThread,
                actual_state: self.thread_state,
            })
        } else {
            Ok(())
        }
    }

    fn check_for_audio_thread(&self) -> Result<(), ClapPluginThreadError> {
        if self.thread_state != ThreadState::AudioThread {
            Err(ClapPluginThreadError {
                requested_state: ThreadState::AudioThread,
                actual_state: self.thread_state,
            })
        } else {
            Ok(())
        }
    }

    #[inline]
    fn is_plugin_active(&self) -> bool {
        match self.state {
            PluginState::Inactive => false,
            PluginState::InactiveWithError => false,
            _ => true,
        }
    }
}

unsafe impl Send for ClapPluginInstance {}
unsafe impl Sync for ClapPluginInstance {}

pub(crate) struct ClapPluginMainThread<'a> {
    plugin: &'a mut ClapPluginInstance,
}

impl<'a> ClapPluginMainThread<'a> {
    pub fn load<P: Into<PathBuf>>(&mut self, path: P) -> Result<(), ()> {
        if self.plugin.is_loaded {
            self.unload();
        }

        todo!()
    }

    pub fn unload(&mut self) -> Result<(), ()> {
        if !self.plugin.is_loaded {
            return Ok(());
        }

        if self.plugin.is_gui_created {
            // TODO: Destory plugin window

            self.plugin.is_gui_created = false;
            self.plugin.is_gui_visible = false;
        }

        self.deactivate();

        // TODO: Destroy plugin

        todo!()
    }

    pub fn can_activate(&self, engine: &RustyDAWEngine) -> bool {
        if !engine.is_running() {
            false
        } else if self.plugin.is_plugin_active() {
            false
        } else if self.plugin.schedule_restart {
            false
        } else {
            true
        }
    }

    pub fn activate(
        &mut self,
        sample_rate: SampleRate,
        min_block_size: u32,
        max_block_size: u32,
    ) -> Result<(), ClapPluginActivationError> {
        if self.plugin.is_plugin_active() {
            return Err(ClapPluginActivationError::PluginAlreadyActivated);
        }

        if let Some(raw_plugin) = self.plugin.raw_plugin {
            unsafe {
                let raw_plugin = raw_plugin.as_ref();

                if !(raw_plugin.activate)(
                    raw_plugin,
                    sample_rate.as_f64(),
                    min_block_size,
                    max_block_size,
                ) {
                    self.plugin.state = PluginState::InactiveWithError;
                    return Err(ClapPluginActivationError::PluginFailure);
                }
            }
        } else {
            return Err(ClapPluginActivationError::PluginNotLoaded);
        }

        self.plugin.schedule_process = true;
        self.plugin.state = PluginState::ActiveAndSleeping;

        Ok(())
    }

    pub fn deactivate(&mut self) {
        if !self.plugin.is_plugin_active() {
            return;
        }

        todo!()
    }

    pub fn set_audio_ports(&mut self, audio_ports: ClapAudioPorts) -> Result<(), ()> {
        // TODO: Assert that these buffers match what the plugin requested?
        self.plugin.audio_ports = audio_ports;

        Ok(())
    }

    pub fn process_begin(self) {
        self.plugin.thread_state = ThreadState::AudioThread;
    }

    pub fn idle(&mut self) {
        todo!()
    }
}

pub(crate) struct ClapPluginAudioThread<'a> {
    plugin: &'a mut ClapPluginInstance,
}

impl<'a> ClapPluginAudioThread<'a> {
    pub fn process_end(&mut self) {
        self.plugin.thread_state = ThreadState::Unknown;
    }

    pub fn process(self, process: &mut clap_process) -> ProcessStatus {
        // Can't process a plugin that is not active.
        if !self.plugin.is_plugin_active() {
            return ProcessStatus::Sleep;
        }

        // This will always be `Some` when the plugin is active because
        // the plugin must be loaded before it can be activated.
        let raw_plugin = self.plugin.raw_plugin.unwrap();

        // Better safe than sorry.
        if raw_plugin.as_ptr().is_null() {
            log::error!("Plugin pointer is null!");
            return ProcessStatus::Error;
        }

        // Do we want to deactivate the plugin?
        if self.plugin.schedule_deactivate {
            self.plugin.schedule_deactivate = false;

            if self.plugin.state == PluginState::ActiveAndProcessing {
                // This is safe becase the raw plugin will always be initialized
                // when the plugin is active.
                unsafe {
                    let raw_plugin = raw_plugin.as_ref();
                    (raw_plugin.stop_processing)(raw_plugin);
                }
            }

            self.plugin.state = PluginState::ActiveAndReadyToDeactivate;
        }

        // We can't process a plugin which failed to start processing.
        if self.plugin.state == PluginState::ActiveWithError {
            return ProcessStatus::Error;
        }

        process.transport = std::ptr::null();

        process.in_events = std::ptr::null();
        process.out_events = std::ptr::null();

        // TODO: event stuff

        if self.plugin.state == PluginState::ActiveAndSleeping {
            if !self.plugin.schedule_process {
                // TODO: Check if there are events.

                // The plugin is sleeping, there is no request to wake it up and
                // there are no events to process
                return ProcessStatus::Sleep;
            }

            self.plugin.schedule_process = false;

            // This is safe becase the raw plugin will always be initialized
            // when the plugin is active.
            unsafe {
                let raw_plugin = raw_plugin.as_ref();
                if !(raw_plugin.start_processing)(raw_plugin) {
                    // The plugin failed to start processing.
                    self.plugin.state = PluginState::ActiveWithError;
                    return ProcessStatus::Error;
                }

                self.plugin.state = PluginState::ActiveAndProcessing;
            }
        }

        let status = if self.plugin.state == PluginState::ActiveAndProcessing {
            // Assign the buffer pointers to the process struct.
            self.plugin.audio_ports.prepare(process);

            // This is safe becase the raw plugin will always be initialized
            // when the plugin is active.
            unsafe {
                let raw_plugin = raw_plugin.as_ref();

                ProcessStatus::from_clap((raw_plugin.process)(raw_plugin, process))
                    .unwrap_or(ProcessStatus::Error)
            }
        } else {
            ProcessStatus::Sleep
        };

        // TODO: event stuff

        status
    }
}

unsafe extern "C" fn get_extension(
    host: *const clap_host,
    extension_id: *const c_char,
) -> *const c_void {
    if host.is_null() {
        log::warn!(
            "Call to `get_extension(host: *const clap_host) received a null pointer from plugin`"
        );
        return ptr::null();
    }

    // What an even more convoluted way to extract the plugin instance from a "*const c_void" pointer :/
    let plugin_instance = &*(&*((*host).host_data as *const UnsafeCell<ClapPluginInstance>)).get();

    todo!()
}

unsafe extern "C" fn request_restart(host: *const clap_host) {
    if host.is_null() {
        log::warn!(
            "Call to `request_restart(host: *const clap_host) received a null pointer from plugin`"
        );
        return;
    }

    let plugin_instance =
        &mut *(&*((*host).host_data as *const UnsafeCell<ClapPluginInstance>)).get();

    todo!()
}

unsafe extern "C" fn request_process(host: *const clap_host) {
    if host.is_null() {
        log::warn!(
            "Call to `request_process(host: *const clap_host) received a null pointer from plugin`"
        );
        return;
    }

    let plugin_instance =
        &mut *(&*((*host).host_data as *const UnsafeCell<ClapPluginInstance>)).get();

    todo!()
}

unsafe extern "C" fn request_callback(host: *const clap_host) {
    if host.is_null() {
        log::warn!(
            "Call to `request_callback(host: *const clap_host) received a null pointer from plugin`"
        );
        return;
    }

    let plugin_instance =
        &mut *(&*((*host).host_data as *const UnsafeCell<ClapPluginInstance>)).get();

    todo!()
}
