use std::error::Error;
use std::fmt;
use std::fs::File;
use std::path::PathBuf;

use basedrop::{Handle, Shared};

use meadowlark_core_types::{Frames, SampleRate};
use symphonia::core::codecs::CodecRegistry;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::{Hint, Probe};

// TODO: Eventually we should use disk streaming to store large files. Using this as a stop-gap
// safety check for now.
pub static MAX_FILE_BYTES: u64 = 1_000_000_000;

use super::{decode, PcmResource, PcmResourceType};
use crate::utils::twox_hash_map::TwoXHashMap;

#[derive(Default, Debug, Clone, PartialEq, Hash, Eq)]
pub struct PcmKey {
    pub path: PathBuf,
    //pub resample_to_project_sr: bool,
    //pub quality: ResampleQuality,

    /* TODO
    /// The amount of doppler stretching to apply.
    ///
    /// By default this is `1.0` (no doppler stretching).
    //pub doppler_stretch_ratio: f64,
     */
}

pub struct PcmLoader {
    loaded: TwoXHashMap<PcmKey, Shared<PcmResource>>,

    /// The resource to send when the resource could not be loaded.
    empty_pcm: Shared<PcmResource>,

    project_sr: SampleRate,

    codec_registry: &'static CodecRegistry,
    probe: &'static Probe,

    coll_handle: Handle,
}

impl PcmLoader {
    pub fn new(coll_handle: Handle, project_sr: SampleRate) -> Self {
        let empty_pcm = Shared::new(
            &coll_handle,
            PcmResource {
                pcm_type: PcmResourceType::F32(vec![Vec::new()]),
                sample_rate: project_sr,
                channels: 1,
                len_frames: Frames(0),
            },
        );

        Self {
            loaded: Default::default(),
            empty_pcm,
            project_sr,
            codec_registry: symphonia::default::get_codecs(),
            probe: symphonia::default::get_probe(),
            coll_handle,
        }
    }

    pub fn load(&mut self, key: &PcmKey) -> (Shared<PcmResource>, Result<(), PcmLoadError>) {
        match self.try_load(key) {
            Ok(pcm) => (pcm, Ok(())),
            Err(e) => {
                log::error!("{}", e);

                // Send an "empty" PCM resource instead.
                (Shared::clone(&self.empty_pcm), Err(e))
            }
        }
    }

    fn try_load(&mut self, key: &PcmKey) -> Result<Shared<PcmResource>, PcmLoadError> {
        log::debug!("Loading PCM file: {:?}", &key.path);

        if let Some(pcm) = self.loaded.get(key) {
            // Resource is already loaded.
            log::debug!("PCM file already loaded");
            return Ok(Shared::clone(pcm));
        }

        // Try to open the file.
        let file =
            File::open(&key.path).map_err(|e| PcmLoadError::PathNotFound((key.path.clone(), e)))?;

        // Create a hint to help the format registry guess what format reader is appropriate.
        let mut hint = Hint::new();

        // Provide the file extension as a hint.
        if let Some(extension) = key.path.extension() {
            if let Some(extension_str) = extension.to_str() {
                hint.with_extension(extension_str);
            }
        }

        // Create the media source stream.
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Use the default options for format reader, metadata reader, and decoder.
        let format_opts: FormatOptions = Default::default();
        let metadata_opts: MetadataOptions = Default::default();

        // Probe the media source stream for metadata and get the format reader.
        let mut probed = self
            .probe
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| PcmLoadError::UnkownFormat((key.path.clone(), e)))?;

        // Get the default track in the audio stream.
        let track = probed
            .format
            .default_track()
            .ok_or_else(|| PcmLoadError::NoTrackFound(key.path.clone()))?;

        let sample_rate = SampleRate(track.codec_params.sample_rate.unwrap_or_else(|| {
            log::warn!("Could not find sample rate of PCM resource at {:?}. Assuming a sample rate of 44100", &key.path);
            44100
        }) as f64);

        /*
        let pcm = if sample_rate == self.project_sr || !key.resample_to_project_sr {
            decode::decode_native_bitdepth(&mut probed, key, self.codec_registry, sample_rate)?
        } else {
            // Resampling is needed.
            decode::decode_f32_resampled(&mut probed, key, self.codec_registry, sample_rate, self.project_sr)?
        };
        */

        let pcm =
            decode::decode_native_bitdepth(&mut probed, key, self.codec_registry, sample_rate)?;

        let pcm = Shared::new(&self.coll_handle, pcm);

        self.loaded.insert(key.to_owned(), Shared::clone(&pcm));

        log::debug!("Successfully loaded PCM file");

        Ok(pcm)
    }

    /// Drop all PCM resources not being currently used.
    pub fn collect(&mut self) {
        // If no other extant Shared pointers to the resource exists, then
        // remove that entry.
        self.loaded.retain(|_, pcm| Shared::get_mut(pcm).is_none());
    }
}

#[derive(Debug)]
pub enum PcmLoadError {
    PathNotFound((PathBuf, std::io::Error)),
    UnkownFormat((PathBuf, symphonia::core::errors::Error)),
    NoTrackFound(PathBuf),
    NoChannelsFound(PathBuf),
    UnkownChannelFormat((PathBuf, usize)),
    FileTooLarge(PathBuf),
    CouldNotCreateDecoder((PathBuf, symphonia::core::errors::Error)),
    ErrorWhileDecoding((PathBuf, symphonia::core::errors::Error)),
    UnexpectedErrorWhileDecoding((PathBuf, Box<dyn Error>)),
    //ErrorWhileResampling((PathBuf, samplerate::Error)),
}

impl Error for PcmLoadError {}

impl fmt::Display for PcmLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use PcmLoadError::*;

        match self {
            PathNotFound((path, e)) => write!(f, "Failed to load PCM resource {:?}: file not found | {}", path, e),
            UnkownFormat((path, e)) => write!(
                f,
                "Failed to load PCM resource: format not supported | {} | path: {:?}",
                e,
                &path,
            ),
            NoTrackFound(path) => write!(f, "Failed to load PCM resource: no default track found | path: {:?}", path),
            NoChannelsFound(path) => write!(f, "Failed to load PCM resource: no channels found | path: {:?}", path),
            UnkownChannelFormat((path, n_channels)) => write!(
                f,
                "Failed to load PCM resource: unkown channel format | {} channels found | path: {:?}",
                n_channels,
                path
            ),
            FileTooLarge(path) => write!(
                f,
                "Failed to load PCM resource: file is too large | maximum is {} bytes | path: {:?}",
                MAX_FILE_BYTES,
                path
            ),
            CouldNotCreateDecoder((path, e)) => write!(
                f,
                "Failed to load PCM resource: failed to create decoder | {} | path: {:?}",
                e,
                path
            ),
            ErrorWhileDecoding((path, e)) => write!(
                f,
                "Failed to load PCM resource: error while decoding | {} | path: {:?}",
                e,
                path
            ),
            UnexpectedErrorWhileDecoding((path, e)) => write!(
                f,
                "Failed to load PCM resource: unexpected error while decoding | {} | path: {:?}",
                e,
                path
            ),
            /*
            ErrorWhileResampling((path, e)) => write!(
                f,
                "Failed to load PCM resource: error while resampling | {} | path: {:?}",
                e,
                path
            ),
            */
        }
    }
}
