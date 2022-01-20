use clap_sys::chmap::{
    clap_chmap, CLAP_CHMAP_AMBISONIC, CLAP_CHMAP_MONO, CLAP_CHMAP_STEREO, CLAP_CHMAP_SURROUND,
    CLAP_CHMAP_UNSPECIFIED,
};

#[non_exhaustive]
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum ChannelMap {
    Unspecified = CLAP_CHMAP_UNSPECIFIED,
    Mono = CLAP_CHMAP_MONO,

    // left, right
    Stereo = CLAP_CHMAP_STEREO,

    // TODO
    Surround = CLAP_CHMAP_SURROUND,

    // TODO
    Ambisonic = CLAP_CHMAP_AMBISONIC,
}

impl ChannelMap {
    pub fn from_clap(status: clap_chmap) -> Option<ChannelMap> {
        match status {
            CLAP_CHMAP_UNSPECIFIED => Some(ChannelMap::Unspecified),
            CLAP_CHMAP_MONO => Some(ChannelMap::Mono),
            CLAP_CHMAP_STEREO => Some(ChannelMap::Stereo),
            CLAP_CHMAP_SURROUND => Some(ChannelMap::Surround),
            CLAP_CHMAP_AMBISONIC => Some(ChannelMap::Ambisonic),
            _ => None,
        }
    }

    pub fn to_clap(&self) -> clap_chmap {
        match self {
            ChannelMap::Unspecified => CLAP_CHMAP_UNSPECIFIED,
            ChannelMap::Mono => CLAP_CHMAP_MONO,
            ChannelMap::Stereo => CLAP_CHMAP_STEREO,
            ChannelMap::Surround => CLAP_CHMAP_SURROUND,
            ChannelMap::Ambisonic => CLAP_CHMAP_AMBISONIC,
        }
    }
}

impl Default for ChannelMap {
    fn default() -> Self {
        ChannelMap::Unspecified
    }
}
