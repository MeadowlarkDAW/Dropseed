pub const PORT_TYPE_MONO: &'static str = "mono";
pub const PORT_TYPE_STEREO: &'static str = "stereo";

#[derive(Debug, Clone)]
/// The layout of the audio ports of a plugin.
///
/// By default this is set to use `MainPortsLayout::StereoInPlace`,
/// `extra_layout: None`, meaning there is one main stereo input port and
/// one main stereo output port which will always use 32 bit (`f32`) buffers.
/// This input/output port pair is an "in_place" pair, meaning the host may
/// send a single buffer for both the main input and output port, akin to
/// `process_replacing()` in VST.
pub struct AudioPortsExtension {
    /// The layout of the main ports. By default this is set to
    /// `MainPortsLayout::StereoInPlace`.
    pub main_layout: MainPortsLayout,

    /// The layout of any extra ports. By default this is set to
    /// `None`.
    pub extra_layout: Option<ExtraPortsLayout>,
}

impl Default for AudioPortsExtension {
    fn default() -> Self {
        AudioPortsExtension { main_layout: MainPortsLayout::default(), extra_layout: None }
    }
}

#[derive(Debug, Clone)]
/// The layout of the main audio ports of a plugin.
pub enum MainPortsLayout {
    /// This plugin has no main input or output ports.
    NoMainPorts,

    /// This plugin has a main stereo input port and a main stereo output
    /// port.
    ///
    /// In addition, these ports are tied together in an "in_place" pair,
    /// meaning this plugin supports processing the input port and output
    /// port in a single buffer, akin to `process_replacing()` in VST.
    ///
    /// Note that the host may still decide to send separate buffers for
    /// the input/output pair.
    ///
    /// These buffers will always be 32 bit (`f32`).
    ///
    /// This is the default option.
    StereoInPlace,

    /// This plugin has a main stereo input port and a main stereo output
    /// port.
    ///
    /// In addition, these ports are tied together in an "in_place" pair,
    /// meaning this plugin supports processing the input port and output
    /// port in a single buffer, akin to `process_replacing()` in VST.
    ///
    /// Note that the host may still decide to send separate buffers for
    /// the input/output pair.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    StereoInPlacePrefers64,

    /// This plugin has a main stereo input port and a main stereo output
    /// port.
    ///
    /// Unlike `MainPortsLayout::StereoInPlace`, the host will always
    /// send separate buffers for the input and output port.
    ///
    /// These buffers will always be 32 bit (`f32`).
    StereoInOut,

    /// This plugin has a main stereo input port and a main stereo output
    /// port.
    ///
    /// Unlike `MainPortsLayout::StereoInPlace`, the host will always
    /// send separate buffers for the input and output port.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    StereoInOutPrefers64,

    /// This plugin only has a main stereo input port and no main output
    /// port.
    ///
    /// These buffers will always be 32 bit (`f32`).
    StereoInOnly,

    /// This plugin only has a main stereo input port and no main output
    /// port.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    StereoInOnlyPrefers64,

    /// This plugin only has a main stereo output port and no main input
    /// port.
    ///
    /// These buffers will always be 32 bit (`f32`).
    StereoOutOnly,

    /// This plugin only has a main stereo output port and no main input
    /// port.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    StereoOutOnlyPrefers64,

    /// This plugin has a main mono input port and a main mono output
    /// port.
    ///
    /// In addition, these ports are tied together in an "in_place" pair,
    /// meaning this plugin supports processing the input port and output
    /// port in a single buffer, akin to `process_replacing()` in VST.
    ///
    /// Note that the host may still decide to send separate buffers for
    /// the input/output pair.
    ///
    /// These buffers will always be 32 bit (`f32`).
    MonoInPlace,

    /// This plugin has a main mono input port and a main mono output
    /// port.
    ///
    /// In addition, these ports are tied together in an "in_place" pair,
    /// meaning this plugin supports processing the input port and output
    /// port in a single buffer, akin to `process_replacing()` in VST.
    ///
    /// Note that the host may still decide to send separate buffers for
    /// the input/output pair.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    MonoInPlacePrefers64,

    /// This plugin has a main mono input port and a main mono output
    /// port.
    ///
    /// Unlike `MainPortsLayout::MonoInPlace`, the host will always
    /// send separate buffers for the input and output port.
    ///
    /// These buffers will always be 32 bit (`f32`).
    MonoInOut,

    /// This plugin has a main mono input port and a main mono output
    /// port.
    ///
    /// Unlike `MainPortsLayout::MonoInPlace`, the host will always
    /// send separate buffers for the input and output port.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    MonoInOutPrefers64,

    /// This plugin only has a main mono input port and no main output
    /// port.
    ///
    /// These buffers will always be 32 bit (`f32`).
    MonoInOnly,

    /// This plugin only has a main mono input port and no main output
    /// port.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    MonoInOnlyPrefers64,

    /// This plugin only has a main mono output port and no main input
    /// port.
    ///
    /// These buffers will always be 32 bit (`f32`).
    MonoOutOnly,

    /// This plugin only has a main mono output port and no main input
    /// port.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    MonoOutOnlyPrefers64,

    /// This plugin has a main mono input port and a main stereo
    /// output port.
    ///
    /// The host will always send separate buffers for the input
    /// and output ports.
    ///
    /// These buffers will always be 32 bit (`f32`).
    MonoInStereoOut,

    /// This plugin has a main mono input port and a main stereo
    /// output port.
    ///
    /// The host will always send separate buffers for the input
    /// and output ports.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    MonoInStereoOutPrefers64,

    /// This plugin has a main stereo input port and a main mono
    /// output port.
    ///
    /// The host will always send separate buffers for the input
    /// and output ports.
    ///
    /// These buffers will always be 32 bit (`f32`).
    StereoInMonoOut,

    /// This plugin has a main stereo input port and a main mono
    /// output port.
    ///
    /// The host will always send separate buffers for the input
    /// and output ports.
    ///
    /// In addition, this tell the host that this plugin prefers to use
    /// 64 bit (`f64`) buffers for this port. Note that the host may still
    /// decide to send 32 bit (`f32`) buffers regardless.
    StereoInMonoOutPrefers64,

    /// Use a custom channel layout for the main ports.
    Custom {
        input: Option<CustomPortInfo>,
        output: Option<CustomPortInfo>,

        /// If `true`, then these main ports are tied together in an
        /// "in_place" pair, meaning this plugin supports processing the
        /// input port and output port in a single buffer, akin to
        /// `process_replacing()` in VST.
        ///
        /// Note that the host may still decide to send separate buffers for
        /// the input/output pair.
        in_place: bool,
    },
}

impl Default for MainPortsLayout {
    fn default() -> Self {
        MainPortsLayout::StereoInPlace
    }
}

impl MainPortsLayout {
    /// The number of channels of the input port and output port.
    ///
    /// `(input port, output port)`
    pub fn num_channels(&self) -> (usize, usize) {
        match self {
            MainPortsLayout::NoMainPorts => (0, 0),

            MainPortsLayout::StereoInPlace => (2, 2),
            MainPortsLayout::StereoInPlacePrefers64 => (2, 2),
            MainPortsLayout::StereoInOut => (2, 2),
            MainPortsLayout::StereoInOutPrefers64 => (2, 2),
            MainPortsLayout::StereoInOnly => (2, 0),
            MainPortsLayout::StereoInOnlyPrefers64 => (2, 0),
            MainPortsLayout::StereoOutOnly => (0, 2),
            MainPortsLayout::StereoOutOnlyPrefers64 => (0, 2),

            MainPortsLayout::MonoInPlace => (1, 1),
            MainPortsLayout::MonoInPlacePrefers64 => (1, 1),
            MainPortsLayout::MonoInOut => (1, 1),
            MainPortsLayout::MonoInOutPrefers64 => (1, 1),
            MainPortsLayout::MonoInOnly => (1, 0),
            MainPortsLayout::MonoInOnlyPrefers64 => (1, 0),
            MainPortsLayout::MonoOutOnly => (0, 1),
            MainPortsLayout::MonoOutOnlyPrefers64 => (0, 1),

            MainPortsLayout::MonoInStereoOut => (1, 2),
            MainPortsLayout::MonoInStereoOutPrefers64 => (1, 2),
            MainPortsLayout::StereoInMonoOut => (2, 1),
            MainPortsLayout::StereoInMonoOutPrefers64 => (2, 1),

            MainPortsLayout::Custom { input, output, .. } => (
                input.as_ref().map(|p| p.channels).unwrap_or(0),
                output.as_ref().map(|p| p.channels).unwrap_or(0),
            ),
        }
    }
}

#[derive(Debug, Clone)]
/// The layout of any extra input and output audio ports. (i.e. sidechain and
/// bus ports)
pub struct ExtraPortsLayout {
    pub extra_inputs: Vec<CustomPortInfo>,
    pub extra_outputs: Vec<CustomPortInfo>,
}

#[derive(Debug, Clone)]
/// Information about a custom audio port.
pub struct CustomPortInfo {
    /// The number of channels in this port.
    ///
    /// This cannot be `0`.
    pub channels: usize,

    /// If `true`, then it tells the host that this plugin prefers to use 64 bit
    /// (`f64`) buffers for this port. Note the host may still decide to send
    /// 32 bit (`f32`) buffers to this port regardless.
    pub prefers_64_bit: bool,

    /// If `None` or empty then it is unspecified (arbitrary audio).
    ///
    /// This can be compared against:
    /// - `PORT_TYPE_MONO`  ("mono")
    /// - `PORT_TYPE_STEREO`  ("stereo")
    /// - `PORT_TYPE_SURROUND` (defined in the surround extension)
    /// - `PORT_TYPE_AMBISONIC` (defined in the ambisonic extension)
    /// - `PORT_TYPE_CV` (defined in the cv extension)
    ///
    /// An extension can provide its own port type and way to inspect the channels.
    pub port_type: Option<String>,

    /// The displayable name.
    ///
    /// Set this to `None` to use the default name.
    pub display_name: Option<String>,
}
