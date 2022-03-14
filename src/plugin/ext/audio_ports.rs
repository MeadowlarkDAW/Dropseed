use bitflags::bitflags;

use clap_sys::ext::audio_ports::{CLAP_AUDIO_PORTS_PREFERS_64BITS, CLAP_AUDIO_PORT_IS_MAIN};
/*
use clap_sys::id::CLAP_INVALID_ID;
use std::borrow::Cow;
use std::default;

use crate::c_char_helpers::{c_char_buf_to_str, c_char_ptr_to_maybe_str};
*/

pub const PORT_TYPE_MONO: &'static str = "mono";
pub const PORT_TYPE_STEREO: &'static str = "stereo";

bitflags! {
    pub struct AudioPortFlags: u32 {
        /// This port is the main audio input or output.
        ///
        /// There can be only one main input and main output.
        /// Main ports **must** be at index 0.
        const IS_MAIN = CLAP_AUDIO_PORT_IS_MAIN;

        /// Prefers to use 64 bit audio with this port.
        const PREFERS_64BIT = CLAP_AUDIO_PORTS_PREFERS_64BITS;
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Information on an audio port.
pub struct AudioPortInfo {
    /// The stable unique identifier of this audio port.
    pub unique_stable_id: u32,

    /// The displayable name.
    pub display_name: String,

    /// Additional bit flags for this audio port.
    pub flags: AudioPortFlags,

    /// The number of channels in this port.
    ///
    /// For example, a mono audio port would have `1` channel, and a
    /// stereo audio port would have `2`.
    pub channel_count: u32,

    /// If `None` or empty then it is unspecified (arbitrary audio).
    ///
    /// This can be compared against:
    /// - PORT_TYPE_MONO
    /// - PORT_TYPE_STEREO
    /// - PORT_TYPE_SURROUND (defined in the surround extension)
    /// - PORT_TYPE_AMBISONIC (defined in the ambisonic extension)
    /// - PORT_TYPE_CV (defined in the cv extension)
    ///
    /// An extension can provide its own port type and way to inspect the channels.
    pub port_type: Option<String>,

    /// In-place processing: allow the host to use the same buffer for input and output.
    ///
    /// If supported set the same unique "pair ID" on the input port and the output
    /// port to be paired.
    ///
    /// If not supported set this to `None`.
    pub in_place_pair_id: Option<u32>,
}

/*
impl AudioPortInfo {
    pub fn from_clap(info: &'a clap_audio_port_info) -> Option<Self> {
        let display_name = match c_char_buf_to_str(&info.name) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Failed to parse name of audio port: {}", e);

                Cow::from("(error)")
            }
        };

        let flags = AudioPortFlags::from_bits(info.flags).unwrap_or(AudioPortFlags::empty());

        let port_type = match c_char_ptr_to_maybe_str(info.port_type, 256) {
            Some(Ok(p)) => Some(p),
            Some(Err(_)) => {
                log::warn!(
                    "Failed to parse audio port type: no null bit found within max of 256 bytes"
                );

                None
            }
            None => None,
        };

        let in_place_pair_id =
            if info.in_place_pair != CLAP_INVALID_ID { Some(info.in_place_pair) } else { None };

        Some(Self {
            unique_stable_id: info.id,
            display_name,
            flags,
            channel_count: info.channel_count,
            port_type,
            in_place_pair_id,
        })
    }
}
*/

#[derive(Debug, Clone, PartialEq)]
pub struct CustomAudioPortLayout {
    pub input_ports: Vec<AudioPortInfo>,
    pub output_ports: Vec<AudioPortInfo>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
/// An extension that allows a plugin to tell the host the configuration
/// of its audio ports.
///
/// By default this is set to `AudioPortLayout::StereoInPlace`.
///
/// When using the default option, the host will always send one of these
/// options the the plugin's `process()` method:
///
/// * `ProcBufferLayout::StereoInPlace32`
/// * `ProcBufferLayout::StereoInOut32`
pub enum AudioPortLayout {
    /// This plugin has a single 32 bit stereo output port.
    ///
    /// The host will always send this option the the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::StereoOut32`
    StereoOut,
    /// This plugin has a single stereo output port, and it prefers this
    /// port to be 64 bit.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoOut32`
    /// * `ProcBufferLayout::StereoOut64`
    StereoOutPrefers64,

    /// This plugin has a single 32 bit mono output port.
    ///
    /// The host will always send this option the the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::MonoOut32`
    MonoOut,
    /// This plugin has a single mono output port, and it prefers this
    /// port to be 64 bit.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoOut32`
    /// * `ProcBufferLayout::MonoOut64`
    MonoOutPrefers64,

    /// This plugin has a 32 bit stereo input and a 32 bit stereo output
    /// port that are tied together in an "in_place" pair, meaning this plugin
    /// supports processing these buffers "in place", akin to `process_replacing()`
    /// in VST.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// This is the default option.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInPlace,
    /// This plugin has a stereo input and a stereo output port that are tied
    /// together in an "in_place" pair, meaning this plugin supports processing
    /// these buffers "in place", akin to `process_replacing()` in VST.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInPlace64`
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInPlace32`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInPlacePrefers64,

    /// This plugin has a 32 bit stereo input and a 32 bit stereo output
    /// port that are tied together in an "in_place" pair, meaning this plugin
    /// supports processing these buffers "in place", akin to `process_replacing()`
    /// in VST.
    ///
    /// This plugin also has an additional 32 bit stereo input port for sidechain.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInPlaceWithSidechain,
    /// This plugin has a stereo input and a stereo output port that are tied
    /// together in an "in_place" pair, meaning this plugin supports processing
    /// these buffers "in place", akin to `process_replacing()` in VST.
    ///
    /// This plugin also has an additional stereo input port for sidechain.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInPlaceWithSidechainPrefers64,

    /// This plugin has a 32 bit stereo input and a 32 bit stereo output
    /// port that are tied together in an "in_place" pair, meaning this plugin
    /// supports processing these buffers "in place", akin to `process_replacing()`
    /// in VST.
    ///
    /// This plugin also has an additional 32 bit stereo output port.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInPlaceWithExtraOut,
    /// This plugin has a stereo input and a stereo output port that are tied
    /// together in an "in_place" pair, meaning this plugin supports processing
    /// these buffers "in place", akin to `process_replacing()` in VST.
    ///
    /// This plugin also has an additional stereo output port.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInPlaceWithExtraOut32`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInPlaceWithExtraOutPrefers64,

    /// This plugin has a 32 bit mono input and a 32 bit mono output
    /// port that are tied together in an "in_place" pair, meaning this plugin
    /// supports processing these buffers "in place", akin to `process_replacing()`
    /// in VST.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInPlace,
    /// This plugin has a mono input and a mono output port that are tied
    /// together in an "in_place" pair, meaning this plugin supports processing
    /// these buffers "in place", akin to `process_replacing()` in VST.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInPlace64`
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInPlace32`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInPlacePrefers64,

    /// This plugin has a 32 bit mono input and a 32 bit mono output
    /// port that are tied together in an "in_place" pair, meaning this plugin
    /// supports processing these buffers "in place", akin to `process_replacing()`
    /// in VST.
    ///
    /// This plugin also has an additional 32 bit mono input port for sidechain.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MononOutWithSidechain32`
    MonoInPlaceWithSidechain,
    /// This plugin has a mono input and a mono output port that are tied
    /// together in an "in_place" pair, meaning this plugin supports processing
    /// these buffers "in place", akin to `process_replacing()` in VST.
    ///
    /// This plugin also has an additional mono input port for sidechain.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send separate buffers for the
    /// input/output pair.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInPlaceWithSidechain32`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInPlaceWithSidechainPrefers64,

    /// This plugin has one 32 bit stereo input port and one 32 bit stereo
    /// output port.
    ///
    /// Unlike `AudioPortLayout::StereoInPlace`, the host will always send
    /// separate buffers for the input and output port.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInOut,
    /// This plugin has one stereo input port and one stereo output port.
    ///
    /// Unlike `AudioPortLayout::StereoInPlacePrefers64`, the host will always
    /// send separate buffers for the input and output port.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInOut64`
    /// * `ProcBufferLayout::StereoInOut32`
    StereoInOutPrefers64,

    /// This plugin has two 32 bit stereo input ports and one 32 bit stereo
    /// output port.
    ///
    /// Unlike `AudioPortLayout::StereoInPlaceWithSidechain`, the host will
    /// always send separate buffers for the input and output ports.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInOutWithSidechain,
    /// This plugin has two stereo input ports and one stereo output port.
    ///
    /// Unlike `AudioPortLayout::StereoInPlaceWithSidechainPrefers64`, the
    /// host will always send separate buffers for the input and output ports.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInOutWithSidechain64`
    /// * `ProcBufferLayout::StereoInOutWithSidechain32`
    StereoInOutWithSidechainPrefers64,

    /// This plugin has one 32 bit stereo input port and two 32 bit stereo
    /// output ports.
    ///
    /// Unlike `AudioPortLayout::StereoInPlaceWithExtraOut`, the host will
    /// always send separate buffers for the input and output ports.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInOutWithExtraOut,
    /// This plugin has one stereo input port and two stereo output ports.
    ///
    /// Unlike `AudioPortLayout::StereoInPlaceWithExtraOutPrefers64`, the
    /// host will always send separate buffers for the input and output port.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInOutWithExtraOut64`
    /// * `ProcBufferLayout::StereoInOutWithExtraOut32`
    StereoInOutWithExtraOutPrefers64,

    /// This plugin has one 32 bit mono input port and one 32 bit mono
    /// output port.
    ///
    /// Unlike `AudioPortLayout::MonoInPlace`, the host will always send
    /// separate buffers for the input and output port.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInOut,
    /// This plugin has one mono input port and one mono output port.
    ///
    /// Unlike `AudioPortLayout::MonoInPlacePrefers64`, the host will always
    /// send separate buffers for the input and output port.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInOut64`
    /// * `ProcBufferLayout::MonoInOut32`
    MonoInOutPrefers64,

    /// This plugin has two 32 bit mono input ports and one 32 bit mono
    /// output port.
    ///
    /// Unlike `AudioPortLayout::MonoInPlaceWithSidechain`, the host will
    /// always send separate buffers for the input and output ports.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInOutWithSidechain,
    /// This plugin has two mono input ports and one mono output port.
    ///
    /// Unlike `AudioPortLayout::MonoInPlaceWithSidechainPrefers64`, the
    /// host will always send separate buffers for the input and output ports.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInOutWithSidechain64`
    /// * `ProcBufferLayout::MonoInOutWithSidechain32`
    MonoInOutWithSidechainPrefers64,

    /// This plugin has one 32 bit mono input port and one 32 bit stereo
    /// output port.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::MonoInStereoOut32`
    MonoInStereoOut,
    /// This plugin has one mono input port and one stereo output port.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::MonoInStereoOut64`
    /// * `ProcBufferLayout::MonoInStereoOut32`
    MonoInStereoOutPrefers64,

    /// This plugin has one 32 bit stereo input port and one 32 bit mono
    /// output port.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::StereoInMonoOut32`
    StereoInMonoOut32,
    /// This plugin has one stereo input port and one mono output port.
    ///
    /// This plugin also prefers to use 64 bit buffers for all its ports.
    ///
    /// Note that the host may still decide to send 32 bit buffers to this
    /// plugin.
    ///
    /// The host will always send one of these options the the plugin's
    /// `process()` method:
    ///
    /// * `ProcBufferLayout::StereoInMonoOut64`
    /// * `ProcBufferLayout::StereoInMonoOut32`
    StereoInMonoOutPrefers64,

    /* TODO
    SurroundOut,
    SurroundOutPrefers64,

    SurroundInPlace,
    SurroundInPlacePrefers64,

    SurroundInPlaceWithSidechain,
    SurroundInPlaceWithSidechainPrefers64,

    SurroundInOut,
    SurroundInOutPrefers64,

    SurroundInOutWithSidechain,
    SurroundInOutWithSidechainPrefers64,
    */
    /// This plugin uses a custom layout of audio ports.
    ///
    /// The host will always send this option to the plugin's `process()`
    /// method:
    ///
    /// * `ProcBufferLayout::Custom`
    Custom(CustomAudioPortLayout),
}

impl Default for AudioPortLayout {
    fn default() -> Self {
        AudioPortLayout::StereoInPlace
    }
}
