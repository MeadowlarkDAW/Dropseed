use std::cell::UnsafeCell;
use std::marker::PhantomPinned;
use std::os::raw::{c_char, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::{error::Error, path::PathBuf};

use basedrop::Shared;
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::version::CLAP_VERSION;
use raw_window_handle::RawWindowHandle;
use rusty_daw_core::SampleRate;

use crate::engine::RustyDAWEngine;
use crate::info::HostInfo;

#[derive(Debug, Clone, Copy, PartialEq)]
enum PluginState {
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
enum ThreadType {
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
                state: PluginState::Inactive,
                raw_plugin: None,
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

    pub fn borrow(&self) -> &ClapPluginInstance {
        unsafe { &*(self.shared.get()) }
    }

    pub fn borrow_mut(&mut self) -> &mut ClapPluginInstance {
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

    state: PluginState,

    raw_plugin: Option<NonNull<clap_plugin>>,

    _phantom_pinned: PhantomPinned,
}

unsafe impl Send for ClapPluginInstance {}
unsafe impl Sync for ClapPluginInstance {}

pub(crate) struct ClapPluginMainThread {
    shared: ClapPluginInstanceShared,
}

impl ClapPluginMainThread {
    pub fn new(
        info: Arc<HostInfo>,
        coll_handle: &basedrop::Handle,
    ) -> (ClapPluginMainThread, ClapPluginAudioThread) {
        let shared = ClapPluginInstanceShared::new(info, coll_handle);

        (
            ClapPluginMainThread {
                shared: shared.clone(),
            },
            ClapPluginAudioThread { shared },
        )
    }

    pub fn load<P: Into<PathBuf>>(&mut self, path: P) -> Result<(), ()> {
        let plugin = self.shared.borrow_mut();

        if plugin.is_loaded {
            self.unload();
        }

        todo!()
    }

    pub fn unload(&mut self) {
        let plugin = self.shared.borrow_mut();

        if !plugin.is_loaded {
            return;
        }

        if plugin.is_gui_created {
            // TODO: Destory plugin window

            plugin.is_gui_created = false;
            plugin.is_gui_visible = false;
        }

        self.deactivate();

        // TODO: Destroy plugin

        todo!()
    }

    pub fn can_activate(&self, engine: &RustyDAWEngine) -> bool {
        let plugin = self.shared.borrow();

        if !engine.is_running() {
            false
        } else if self.is_plugin_active() {
            false
        } else if plugin.schedule_restart {
            false
        } else {
            true
        }
    }

    pub fn activate(&mut self, sample_rate: SampleRate, min_block_size: u32, max_block_size: u32) {
        assert!(!self.is_plugin_active());

        let plugin = self.shared.borrow_mut();

        assert!(plugin.raw_plugin.is_some());

        if let Some(raw_plugin) = plugin.raw_plugin {
            unsafe {
                let raw_plugin = raw_plugin.as_ref();

                if !(raw_plugin.activate)(
                    raw_plugin,
                    sample_rate.as_f64(),
                    min_block_size,
                    max_block_size,
                ) {
                    self.set_plugin_state(PluginState::InactiveWithError);
                    return;
                }
            }
        }

        plugin.schedule_process = true;
        self.set_plugin_state(PluginState::ActiveAndSleeping);
    }

    pub fn deactivate(&mut self) {
        if !self.is_plugin_active() {
            return;
        }

        todo!()
    }

    pub fn is_plugin_active(&self) -> bool {
        let plugin = self.shared.borrow();

        match plugin.state {
            PluginState::Inactive => false,
            PluginState::InactiveWithError => false,
            _ => true,
        }
    }

    fn set_plugin_state(&mut self, state: PluginState) {
        let plugin = self.shared.borrow_mut();

        match state {
            PluginState::Inactive => {
                assert_eq!(plugin.state, PluginState::ActiveAndReadyToDeactivate);
            }
            PluginState::InactiveWithError => {
                assert_eq!(plugin.state, PluginState::Inactive);
            }
            PluginState::ActiveAndSleeping => {
                assert!(
                    plugin.state == PluginState::Inactive
                        || plugin.state == PluginState::ActiveAndProcessing
                );
            }
            PluginState::ActiveAndProcessing => {
                assert_eq!(plugin.state, PluginState::ActiveAndSleeping);
            }
            PluginState::ActiveWithError => {
                assert_eq!(plugin.state, PluginState::ActiveAndProcessing);
            }
            PluginState::ActiveAndReadyToDeactivate => {
                assert!(
                    plugin.state == PluginState::ActiveAndProcessing
                        || plugin.state == PluginState::ActiveAndSleeping
                        || plugin.state == PluginState::ActiveWithError
                );
            }
        }

        plugin.state = state;
    }
}

pub(crate) struct ClapPluginAudioThread {
    shared: ClapPluginInstanceShared,
}

impl ClapPluginAudioThread {}

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
