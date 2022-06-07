use clap_sys::ext::params::clap_param_info as RawClapParamInfo;
use clap_sys::ext::params::clap_plugin_params as RawClapPluginParams;
use clap_sys::ext::params::CLAP_EXT_PARAMS;
use clap_sys::plugin::clap_plugin as RawClapPlugin;
use clap_sys::string_sizes::CLAP_NAME_SIZE;

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::ptr;

use super::c_char_helpers::c_char_buf_to_str;
use super::events::{ClapInputEvents, ClapOutputEvents};
use crate::plugin::ext::params::{ParamID, ParamInfo, ParamInfoFlags};

#[derive(Clone)]
pub struct ClapPluginParams {
    is_some: bool,

    raw: *const RawClapPluginParams,
    raw_plugin: *const RawClapPlugin,
}

impl ClapPluginParams {
    /// [thread-safe]
    pub fn new(raw_plugin: *const RawClapPlugin) -> (Self, bool) {
        assert!(!raw_plugin.is_null());

        let res = unsafe {
            ((*raw_plugin).get_extension)(raw_plugin, CLAP_EXT_PARAMS) as *const RawClapPluginParams
        };

        if res.is_null() {
            (Self { is_some: false, raw: ptr::null(), raw_plugin: ptr::null() }, false)
        } else {
            (Self { is_some: true, raw: res, raw_plugin }, true)
        }
    }

    /// [main-thread]
    pub fn num_params(&self) -> u32 {
        if self.is_some {
            unsafe { ((*self.raw).count)(self.raw_plugin) }
        } else {
            0
        }
    }

    /// [main-thread]
    pub fn param_info(&self, param_index: usize) -> Result<ParamInfo, ()> {
        if !self.is_some {
            return Err(());
        }

        let mut raw_info: RawClapParamInfo = unsafe { MaybeUninit::uninit().assume_init() };
        raw_info.cookie = ptr::null_mut();

        let res =
            unsafe { ((*self.raw).get_info)(self.raw_plugin, param_index as u32, &mut raw_info) };

        if !res {
            log::warn!(
                "Plugin returned `false` in call to clap_plugin_params.get_info() on parameter at index {}",
                param_index
            );
            return Err(());
        }

        let flags = ParamInfoFlags::from_bits_truncate(raw_info.flags);

        let display_name = match c_char_buf_to_str(&raw_info.name) {
            Ok(name) => name.to_string(),
            Err(_) => {
                log::warn!(
                    "Failed to parse clap_param_info.name in call to clap_plugin_params.get_info() on parameter at index {}",
                    param_index
                );
                String::from("(unkown)")
            }
        };

        let module = match c_char_buf_to_str(&raw_info.module) {
            Ok(module) => module.to_string(),
            Err(_) => {
                log::warn!(
                    "Failed to parse clap_param_info.module in call to clap_plugin_params.get_info() on parameter at index {}",
                    param_index
                );
                String::new()
            }
        };

        Ok(ParamInfo {
            stable_id: ParamID::new(raw_info.id),
            flags,
            display_name,
            module,
            min_value: raw_info.min_value,
            max_value: raw_info.max_value,
            default_value: raw_info.default_value,
            cookie: raw_info.cookie,
        })
    }

    /// [main-thread]
    pub fn param_value(&self, param_id: ParamID) -> Result<f64, ()> {
        if !self.is_some {
            return Err(());
        }

        let mut value: f64 = 0.0;

        let res = unsafe { ((*self.raw).get_value)(self.raw_plugin, param_id.0, &mut value) };

        if !res {
            log::warn!(
                "Plugin returned `false` in call to clap_plugin_params.get_value() on parameter with id {:?}",
                param_id
            );
            return Err(());
        }

        Ok(value)
    }

    /// [main-thread]
    pub fn param_value_to_text(&self, param_id: ParamID, value: f64) -> Result<String, ()> {
        if !self.is_some {
            return Err(());
        }

        let mut char_buf: [i8; CLAP_NAME_SIZE] = unsafe { MaybeUninit::uninit().assume_init() };

        let res = unsafe {
            ((*self.raw).value_to_text)(
                self.raw_plugin,
                param_id.0,
                value,
                char_buf.as_mut_ptr(),
                char_buf.len() as u32,
            )
        };

        if !res {
            log::warn!(
                "Plugin returned `false` in call to clap_plugin_params.value_to_text() on parameter with id {:?}",
                param_id
            );
            return Err(());
        }

        match c_char_buf_to_str(&char_buf) {
            Ok(text) => Ok(text.to_string()),
            Err(_) => {
                log::warn!(
                    "Failed to parse char *display in call to clap_plugin_params.value_to_text() on parameter with id {:?}",
                    param_id
                );
                Err(())
            }
        }
    }

    /// [main-thread]
    pub fn param_text_to_value(&self, param_id: ParamID, display: &str) -> Result<f64, ()> {
        if !self.is_some {
            return Err(());
        }

        let mut value: f64 = 0.0;

        let cstr = match CString::new(display) {
            Ok(cstr) => cstr,
            Err(e) => {
                log::error!(
                    "Failed to turn {} into a c_str buf in call to ClapPluginParams::param_text_to_value() on parameter with id {:?}: {}",
                    display,
                    param_id,
                    e,
                );
                return Err(());
            }
        };

        let res = unsafe {
            ((*self.raw).text_to_value)(self.raw_plugin, param_id.0, cstr.as_ptr(), &mut value)
        };

        if !res {
            log::warn!(
                "Plugin returned `false` in call to clap_plugin_params.text_to_value() on parameter with id {:?} with text {}",
                param_id,
                display,
            );
            return Err(());
        }

        Ok(value)
    }

    /// [active && !processing : audio-thread]
    /// [!active : main-thread]
    pub fn param_flush(
        &self,
        clap_in_events: &ClapInputEvents,
        clap_out_events: &ClapOutputEvents,
    ) {
        let raw_in_events = clap_in_events.raw();
        let raw_out_events = clap_out_events.raw();

        unsafe { ((*self.raw).flush)(self.raw_plugin, raw_in_events, raw_out_events) }
    }
}
