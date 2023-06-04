use bitflags::bitflags;

use super::StableID;

pub static PORT_MONO: &'static str = "mono";
pub static PORT_STEREO: &'static str = "stereo";
pub static PORT_SURROUND: &'static str = "surround";
pub static PORT_AMBISONIC: &'static str = "ambisonic";
pub static PORT_CV: &'static str = "cv";

bitflags! {
    /// Bit flags describing an audio port.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AudioPortFlags: u32 {
        /// This port is the main audio input or output.
        ///
        /// There can be only one main input and main output.
        ///
        /// Main port must be at index 0.
        const IS_MAIN = 1 << 0;

        /// This port can be used with 64 bits audio
        const SUPPORTS_64BITS = 1 << 1;

        /// 64 bits audio is preferred with this port
        const PREFERS_64BITS = 1 << 2;

        /// This port must be used with the same sample size as all the other ports which have this flag.
        ///
        /// In other words if all ports have this flag then the plugin may either be used entirely with
        /// 64 bits audio or 32 bits audio, but it can't be mixed.
        const REQUIRES_COMMON_SAMPLE_SIZE = 1 << 3;
    }
}

/// Information about an audio port on a node.
#[derive(Debug, Clone)]
pub struct AudioPortInfo {
    /// id identifies a port and must be stable.
    ///
    /// id may overlap between input and output ports.
    pub id: StableID,

    /// The displayable name for this port.
    pub name: Option<String>,

    /// Additional flags describing this port.
    pub flags: AudioPortFlags,

    /// The number of channels in this port.
    pub channel_count: u32,

    /// A string describing the type of port.
    ///
    /// If this is `None`, then it means it is unspecified (arbitrary audio).
    ///
    /// For example:
    /// * `Some(PORT_MONO.into())`,
    /// * `Some(PORT_STEREO.into())`,
    /// * `Some(PORT_SURROUND.into())`,
    /// * `Some(PORT_AMBISONIC.into())`,
    /// * `Some(PORT_CV.into())`,
    pub port_type: Option<String>,
    // Note, we don't need an `in_place_pair` id because our graph never uses in-place buffers.
}
