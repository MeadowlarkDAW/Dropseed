pub const PORT_TYPE_MONO: &'static str = "mono";
pub const PORT_TYPE_STEREO: &'static str = "stereo";

pub const PORT_NAME_SIDECHAIN: &'static str = "sidechain";

#[derive(Debug, Clone, PartialEq)]
/// The layout of the audio ports of a plugin.
///
/// By default this returns a configuration with a main stereo
/// input port and a main stereo output port.
pub struct AudioPortsExtension {
    /// The list of input audio ports, in order.
    pub inputs: Vec<AudioPortInfo>,

    /// The list of output audio ports, in order.
    pub outputs: Vec<AudioPortInfo>,

    /// Specifies which audio ports are "main" ports.
    pub main_ports_layout: MainPortsLayout,
}

impl Default for AudioPortsExtension {
    fn default() -> Self {
        AudioPortsExtension::stereo_in_out()
    }
}

impl AudioPortsExtension {
    pub fn total_in_out_channels(&self) -> (usize, usize) {
        let mut num_inputs = 0;
        let mut num_outputs = 0;

        for input in self.inputs.iter() {
            num_inputs += input.channels;
        }
        for output in self.outputs.iter() {
            num_outputs += output.channels;
        }

        (num_inputs, num_outputs)
    }

    /// A main stereo input port and a main stereo output port.
    pub fn stereo_in_out() -> Self {
        AudioPortsExtension {
            inputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::InOut,
        }
    }

    /// A main mono input port and a main mono output port.
    pub fn mono_in_out() -> Self {
        AudioPortsExtension {
            inputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::InOut,
        }
    }

    /// A main stereo output port only.
    pub fn stereo_out() -> Self {
        AudioPortsExtension {
            inputs: vec![],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::OutOnly,
        }
    }

    /// A main mono output port only.
    pub fn mono_out() -> Self {
        AudioPortsExtension {
            inputs: vec![],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::OutOnly,
        }
    }

    /// A main stereo input port and a main stereo output port, with an
    /// additional stereo sidechain input.
    pub fn stereo_in_out_w_sidechain() -> Self {
        AudioPortsExtension {
            inputs: vec![
                AudioPortInfo {
                    stable_id: 0,
                    channels: 2,
                    port_type: Some(PORT_TYPE_STEREO.into()),
                    display_name: None,
                },
                AudioPortInfo {
                    stable_id: 1,
                    channels: 2,
                    port_type: Some(PORT_TYPE_STEREO.into()),
                    display_name: Some(PORT_NAME_SIDECHAIN.into()),
                },
            ],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::InOut,
        }
    }

    /// A main mono input port and a main mono output port, with an
    /// additional mono sidechain input.
    pub fn mono_in_out_w_sidechain() -> Self {
        AudioPortsExtension {
            inputs: vec![
                AudioPortInfo {
                    stable_id: 0,
                    channels: 1,
                    port_type: Some(PORT_TYPE_MONO.into()),
                    display_name: None,
                },
                AudioPortInfo {
                    stable_id: 1,
                    channels: 1,
                    port_type: Some(PORT_TYPE_MONO.into()),
                    display_name: Some(PORT_NAME_SIDECHAIN.into()),
                },
            ],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::InOut,
        }
    }

    /// A main stereo output port with an additional stereo sidechain
    /// input.
    pub fn stereo_out_w_sidechain() -> Self {
        AudioPortsExtension {
            inputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: Some(PORT_NAME_SIDECHAIN.into()),
            }],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::OutOnly,
        }
    }

    /// A main mono output port with an additional mono sidechain
    /// input.
    pub fn mono_out_w_sidechain() -> Self {
        AudioPortsExtension {
            inputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: Some(PORT_NAME_SIDECHAIN.into()),
            }],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::OutOnly,
        }
    }

    /// A main mono input port and a main stereo output port.
    pub fn mono_in_stereo_out() -> Self {
        AudioPortsExtension {
            inputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::InOut,
        }
    }

    /// A main stereo input port and a main mono output port.
    pub fn stereo_in_mono_out() -> Self {
        AudioPortsExtension {
            inputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 2,
                port_type: Some(PORT_TYPE_STEREO.into()),
                display_name: None,
            }],
            outputs: vec![AudioPortInfo {
                stable_id: 0,
                channels: 1,
                port_type: Some(PORT_TYPE_MONO.into()),
                display_name: None,
            }],
            main_ports_layout: MainPortsLayout::InOut,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Information about a custom audio port.
pub struct AudioPortInfo {
    /// Stable identifier
    pub stable_id: u32,

    /// The number of channels in this port.
    ///
    /// This cannot be `0`.
    pub channels: usize,

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

#[derive(Debug, Clone, Copy, PartialEq)]
/// Specifies which audio ports are "main" ports.
pub enum MainPortsLayout {
    /// Both the first input port and the first output port are main ports.
    InOut,

    /// The first input port is a main port, and there are no main output ports.
    InOnly,

    /// The first output port is a main port, and there are no main input ports.
    OutOnly,

    /// There are no main input or output ports.
    NoMainPorts,
}

impl Default for MainPortsLayout {
    fn default() -> Self {
        MainPortsLayout::InOut
    }
}
