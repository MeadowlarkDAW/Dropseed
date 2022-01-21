use clap_sys::ext::audio_ports::clap_audio_port_info;
use std::borrow::Cow;

use crate::c_char_helpers::c_char_buf_to_str;
use crate::channel_map::ChannelMap;

/// Specifies whether a port uses floats (`f32`) or doubles (`f64`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum SampleSize {
    F32 = 32,
    F64 = 64,
}

impl SampleSize {
    fn from_clap(sample_size: u32) -> Option<SampleSize> {
        match sample_size {
            32 => Some(SampleSize::F32),
            64 => Some(SampleSize::F64),
            _ => None,
        }
    }

    fn to_clap(&self) -> u32 {
        match self {
            SampleSize::F32 => 32,
            SampleSize::F64 => 64,
        }
    }
}

impl Default for SampleSize {
    fn default() -> Self {
        SampleSize::F32
    }
}

/// Information on an audio port.
pub struct AudioPortInfo<'a> {
    /// The stable unique identifier of this audio port.
    pub unique_stable_id: u32,

    /// The displayable name
    pub display_name: Cow<'a, str>,

    /// The number of channels in this port.
    ///
    /// For example, a mono audio port would have `1` channel, and a
    /// stereo audio port would have `2`.
    pub channel_count: u32,

    /// The channel map of this port.
    pub channel_map: ChannelMap,

    /// Whether or not this port is a "control voltage" port.
    pub is_cv: bool,
}

impl<'a> AudioPortInfo<'a> {
    pub fn from_clap(info: &'a clap_audio_port_info) -> Option<Self> {
        let channel_map = if let Some(m) = ChannelMap::from_clap(info.channel_map) {
            m
        } else {
            log::error!("Failed to parse channel map of audio port. Got: {}", info.channel_map);

            return None;
        };

        /*
        let sample_size = if let Some(s) = SampleSize::from_clap(info.sample_size) {
            s
        } else {
            log::error!(
                "Failed to parse sample size of audio port. Got: {}",
                info.channel_map
            );

            return None;
        };
        */

        let display_name = match c_char_buf_to_str(&info.name) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Failed to parse name of audio port: {}", e);

                Cow::from("(error)")
            }
        };

        Some(Self {
            unique_stable_id: info.id,
            display_name,
            channel_count: info.channel_count,
            channel_map,
            is_cv: info.is_cv,
        })
    }
}

/// The configuration of the "main" audio ports.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MainPortConfig {
    /// Neither of the first input/output ports are a "main" port.
    None = 0,

    /// The first input port is a "main" port, but the first output port
    /// is not a "main" port.
    InputOnly = 1,

    /// The first output port is a "main" port, but the first input port
    /// is not a "main" port.
    OutputOnly = 2,

    /// Both the first input port and the first output port are "main" ports,
    /// but the host must not use the same buffer for both ports.
    Both = 3,

    /// Both the first input port and the first output port are "main" ports,
    /// and also the host may use the same buffer for both ports.
    BothInPlace = 4,
}

/// An extension that allows a plugin to tell the host the configuration
/// of its audio ports.
pub struct PluginAudioPortsExtension<'a> {
    /// The list of audio input ports.
    pub input_ports: &'a [AudioPortInfo<'a>],

    /// The list of audio output ports.
    pub output_ports: &'a [AudioPortInfo<'a>],

    /// The configuration of the "main" audio ports.
    pub main_port_config: MainPortConfig,

    /// Specifies whether the audio buffers use floats or doubles.
    pub sample_size: SampleSize,
}
