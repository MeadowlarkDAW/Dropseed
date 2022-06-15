use bitflags::bitflags;
use std::ffi::c_void;
use std::hash::Hash;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParamID(pub(crate) u32);

impl ParamID {
    pub const fn new(stable_id: u32) -> Self {
        Self(stable_id)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

bitflags! {
    pub struct ParamInfoFlags: u32 {
        /// Is this param stepped? (integer values only)
        ///
        /// If so the double value is converted to integer using a cast (equivalent to trunc).
        const IS_STEPPED = 1 << 0;

        /// Useful for for periodic parameters like a phase.
        const IS_PERIODIC = 1 << 1;

        /// The parameter should not be shown to the user, because it is currently not used.
        ///
        /// It is not necessary to process automation for this parameter.
        const IS_HIDDEN = 1 << 2;

        /// The parameter can't be changed by the host.
        const IS_READONLY = 1 << 3;

        /// This parameter is used to merge the plugin and host bypass button.
        ///
        /// It implies that the parameter is stepped.
        ///
        /// - min: 0 -> bypass off
        /// - max: 1 -> bypass on
        const IS_BYPASS = 1 << 4;

        /// When set:
        /// - automation can be recorded
        /// - automation can be played back
        ///
        /// The host can send live user changes for this parameter regardless of this flag.
        ///
        /// If this parameters affect the internal processing structure of the plugin, ie: max delay, fft
        /// size, ... and the plugins needs to re-allocate its working buffers, then it should call
        /// host->request_restart(), and perform the change once the plugin is re-activated.
        const IS_AUTOMATABLE = 1 << 5;

        /// Does this param support per note automations?
        const IS_AUTOMATABLE_PER_NOTE_ID = 1 << 6;

        /// Does this param support per note automations?
        const IS_AUTOMATABLE_PER_KEY = 1 << 7;

        /// Does this param support per channel automations?
        const IS_AUTOMATABLE_PER_CHANNEL = 1 << 8;

        /// Does this param support per port automations?
        const IS_AUTOMATABLE_PER_PORT = 1 << 9;

        /// Does the parameter support the modulation signal?
        const IS_MODULATABLE = 1 << 10;

        /// Does this param support per note automations?
        const IS_MODULATABLE_PER_NOTE_ID = 1 << 11;

        /// Does this param support per note automations?
        const IS_MODULATABLE_PER_KEY = 1 << 12;

        /// Does this param support per channel automations?
        const IS_MODULATABLE_PER_CHANNEL = 1 << 13;

        /// Does this param support per channel automations?
        const IS_MODULATABLE_PER_PORT = 1 << 14;

        /// Any change to this parameter will affect the plugin output and requires to be done via
        /// process() if the plugin is active.
        ///
        /// A simple example would be a DC Offset, changing it will change the output signal and must be
        /// processed.
        const REQUIRES_PROCESS = 1 << 15;
    }
}

impl ParamInfoFlags {
    /// `Self::IS_AUTOMATABLE | Self::IS_MODULATABLE`
    pub fn default_float() -> Self {
        Self::IS_AUTOMATABLE | Self::IS_MODULATABLE
    }

    /// `Self::IS_STEPPED | Self::IS_AUTOMATABLE | Self::IS_MODULATABLE`
    pub fn default_enum() -> Self {
        Self::IS_STEPPED | Self::IS_AUTOMATABLE | Self::IS_MODULATABLE
    }
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    /// Stable parameter identifier, it must never change.
    pub stable_id: ParamID,

    pub flags: ParamInfoFlags,

    /// The name of this parameter displayed to the user.
    pub display_name: String,

    /// The module containing the param.
    ///
    /// eg: `"oscillators/wt1"`
    ///
    /// `/` will be used as a separator to show a tree like structure.
    pub module: String,

    /// Minimum plain value.
    pub min_value: f64,
    /// Maximum plain value.
    pub max_value: f64,
    /// Default plain value.
    pub default_value: f64,

    /// Reserved for CLAP plugins.
    #[allow(unused)]
    pub(crate) cookie: *const c_void,
}

unsafe impl Send for ParamInfo {}
unsafe impl Sync for ParamInfo {}

impl ParamInfo {
    /// Create info for a parameter.
    ///
    /// - `stable_id` - Stable parameter identifier, it must never change.
    /// - `flags` - Additional flags.
    /// - `display_name` - The name of this parameter displayed to the user.
    /// - `module` - The module containing the param.
    ///     - eg: `"oscillators/wt1"`
    ///     - `/` will be used as a separator to show a tree like structure.
    /// - `min_value`: Minimum plain value.
    /// - `max_value`: Maximum plain value.
    /// - `default_value`: Default plain value.
    pub fn new(
        stable_id: ParamID,
        flags: ParamInfoFlags,
        display_name: String,
        module: String,
        min_value: f64,
        max_value: f64,
        default_value: f64,
    ) -> Self {
        Self {
            stable_id,
            flags,
            display_name,
            module,
            min_value,
            max_value,
            default_value,
            cookie: std::ptr::null(),
        }
    }
}

bitflags! {
    pub struct ParamRescanFlags: u32 {
        /// The parameter values did change (eg. after loading a preset).
        ///
        /// The host will scan all the parameters value.
        ///
        /// The host will not record those changes as automation points.
        ///
        /// New values takes effect immediately.
        const RESCAN_VALUES = 1 << 0;

        /// The value to text conversion changed, and the text needs to be rendered again.
        const RESCAN_TEXT = 1 << 1;

        /// The parameter info did change, use this flag for:
        /// - name change
        /// - module change
        /// - is_periodic (flag)
        /// - is_hidden (flag)
        ///
        /// New info takes effect immediately.
        const RESCAN_INFO = 1 << 2;

        /// Invalidates everything the host knows about parameters.
        ///
        /// It can only be used while the plugin is deactivated.
        ///
        /// If the plugin is activated use clap_host->restart() and delay any change until the host calls
        /// clap_plugin->deactivate().
        ///
        /// You must use this flag if:
        /// - some parameters were added or removed.
        /// - some parameters had critical changes:
        ///   - is_per_note (flag)
        ///  - is_per_channel (flag)
        ///   - is_readonly (flag)
        ///   - is_bypass (flag)
        ///   - is_stepped (flag)
        ///   - is_modulatable (flag)
        ///   - min_value
        ///   - max_value
        ///   - cookie
        const RESCAN_ALL = 1 << 3;
    }
}

bitflags! {
    pub struct ParamClearFlags: u32 {
        /// Clears all possible references to a parameter
        const CLEAR_ALL = 1 << 0;

        /// Clears all automations to a parameter
        const CLEAR_AUTOMATIONS = 1 << 1;

        /// Clears all modulations to a parameter
        const CLEAR_MODULATIONS = 1 << 2;
    }
}

pub struct HostParamsExtMainThread {
    pub(crate) rescan_requested: Arc<(AtomicBool, AtomicU32)>,
    pub(crate) clear_requested: Arc<AtomicBool>,
    pub(crate) flush_requested: Arc<AtomicBool>,
}

impl Clone for HostParamsExtMainThread {
    fn clone(&self) -> Self {
        Self {
            rescan_requested: Arc::clone(&self.rescan_requested),
            clear_requested: Arc::clone(&self.clear_requested),
            flush_requested: Arc::clone(&self.flush_requested),
        }
    }
}

impl HostParamsExtMainThread {
    pub(crate) fn new() -> Self {
        Self {
            rescan_requested: Arc::new((AtomicBool::new(false), AtomicU32::new(0))),
            clear_requested: Arc::new(AtomicBool::new(false)),
            flush_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Rescan the full list of parameters according to the flags.
    ///
    /// [main-thread]
    pub fn rescan(&self, rescan_flags: ParamRescanFlags) {
        let flags = rescan_flags.bits();

        self.rescan_requested.1.store(flags, Ordering::SeqCst);
        self.rescan_requested.0.store(true, Ordering::SeqCst);
    }

    /// Clears references to a parameter.
    ///
    /// [main-thread]
    pub fn clear(&self, param_id: ParamID, clear_flags: ParamClearFlags) {
        // TODO
        log::info!(
            "got request to clear param with id {:?} and flags: {:?}",
            param_id,
            clear_flags
        );
    }

    /// Request the host to call clap_plugin_params->fush().
    /// This is useful if the plugin has parameters value changes to report to the host but the plugin
    /// is not processing.
    ///
    /// eg. the plugin has a USB socket to some hardware controllers and receives a parameter change
    /// while it is not processing.
    ///
    /// This must not be called on the [audio-thread].
    ///
    /// [thread-safe]
    pub fn request_flush(&self) {
        self.flush_requested.store(true, Ordering::SeqCst);
    }
}

pub struct HostParamsExtAudioThread {
    pub(crate) flush_requested: Arc<AtomicBool>,
}

impl HostParamsExtAudioThread {
    /// Request the host to call clap_plugin_params->fush().
    /// This is useful if the plugin has parameters value changes to report to the host but the plugin
    /// is not processing.
    ///
    /// eg. the plugin has a USB socket to some hardware controllers and receives a parameter change
    /// while it is not processing.
    ///
    /// This must not be called on the [audio-thread].
    ///
    /// [thread-safe]
    pub fn request_flush(&self) {
        self.flush_requested.store(true, Ordering::SeqCst);
    }
}
