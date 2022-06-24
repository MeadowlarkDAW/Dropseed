use std::error::Error;
use std::fmt;
use std::fs::File;
use std::path::PathBuf;

use basedrop::{Handle, Shared};

use meadowlark_core_types::{Frame, SampleRate};
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::audio::Signal;
use symphonia::core::codecs::{CodecRegistry, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::{Hint, Probe};

// TODO: Eventually we should use disk streaming to store large files. Using this as a stop-gap
// safety check for now.
pub static MAX_FILE_BYTES: u64 = 1_000_000_000;

use super::{decode, PcmResource, PcmResourceType};
use crate::utils::twox_hash_map::TwoXHashMap;

pub struct PcmLoader {
    loaded: TwoXHashMap<PathBuf, Shared<PcmResource>>,

    /// The resource to send when the resource could not be loaded.
    empty_pcm: Shared<PcmResource>,

    codec_registry: &'static CodecRegistry,
    probe: &'static Probe,

    coll_handle: Handle,
}

impl PcmLoader {
    pub fn new(coll_handle: Handle, sample_rate: SampleRate) -> Self {
        let empty_pcm = Shared::new(
            &coll_handle,
            PcmResource {
                pcm_type: PcmResourceType::F32(vec![Vec::new()]),
                sample_rate,
                channels: 1,
                len_frames: Frame(0),
            },
        );

        Self {
            loaded: Default::default(),
            empty_pcm,
            codec_registry: symphonia::default::get_codecs(),
            probe: symphonia::default::get_probe(),
            coll_handle,
        }
    }

    pub fn load(&mut self, path: &PathBuf) -> (Shared<PcmResource>, Result<(), PcmLoadError>) {
        match self.try_load(path) {
            Ok(pcm) => (pcm, Ok(())),
            Err(e) => {
                log::error!("{}", e);

                // Send an "empty" PCM resource instead.
                (Shared::clone(&self.empty_pcm), Err(e))
            }
        }
    }

    fn try_load(&mut self, path: &PathBuf) -> Result<Shared<PcmResource>, PcmLoadError> {
        log::debug!("Loading PCM file: {:?}", path);

        if let Some(pcm) = self.loaded.get(path) {
            // Resource is already loaded.
            log::debug!("PCM file already loaded");
            return Ok(Shared::clone(pcm));
        }

        // Try to open the file.
        let file = File::open(path).map_err(|e| PcmLoadError::PathNotFound((path.clone(), e)))?;

        // Create a hint to help the format registry guess what format reader is appropriate.
        let mut hint = Hint::new();

        // Provide the file extension as a hint.
        if let Some(extension) = path.extension() {
            if let Some(extension_str) = extension.to_str() {
                hint.with_extension(extension_str);
            }
        }

        // Create the media source stream.
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Use the default options for format reader, metadata reader, and decoder.
        let format_opts: FormatOptions = Default::default();
        let metadata_opts: MetadataOptions = Default::default();
        let decode_opts: DecoderOptions = Default::default();

        // Probe the media source stream for metadata and get the format reader.
        let mut probed = self
            .probe
            .format(&hint, mss, &format_opts, &metadata_opts)
            .map_err(|e| PcmLoadError::UnkownFormat((path.clone(), e)))?;

        // Get the default track in the audio stream.
        let track = probed
            .format
            .default_track()
            .ok_or_else(|| PcmLoadError::NoTrackFound(path.clone()))?;
        let track_id = track.id;

        // Get info.
        let n_channels = track
            .codec_params
            .channels
            .ok_or_else(|| PcmLoadError::NoChannelsFound(path.clone()))?
            .count();

        if n_channels == 0 {
            return Err(PcmLoadError::NoChannelsFound(path.clone()));
        }

        let sample_rate = SampleRate(track.codec_params.sample_rate.unwrap_or_else(|| {
            log::warn!("Could not find sample rate of PCM resource at {:?}. Assuming a sample rate of 44100", &path);
            44100
        }) as f64);

        let n_frames = track.codec_params.n_frames;

        // Eventually we should use disk streaming to store large files. Using this as a stop-gap
        // safety check for now.
        if let Some(n_frames) = n_frames {
            let total_bytes = n_channels as u64 * n_frames * 4;
            if total_bytes > MAX_FILE_BYTES {
                return Err(PcmLoadError::FileTooLarge(path.clone()));
            }
        }

        // Create a decoder for the track.
        let mut decoder = self
            .codec_registry
            .make(&track.codec_params, &decode_opts)
            .map_err(|e| PcmLoadError::CouldNotCreateDecoder((path.clone(), e)))?;

        let max_frames = MAX_FILE_BYTES / (4 * n_channels as u64);
        let mut total_frames = 0;

        enum FirstPacketType {
            U8(Vec<Vec<u8>>),
            U16(Vec<Vec<u16>>),
            U24(Vec<Vec<[u8; 3]>>),
            U32(Vec<Vec<f32>>),
            S8(Vec<Vec<i8>>),
            S16(Vec<Vec<i16>>),
            S24(Vec<Vec<[u8; 3]>>),
            S32(Vec<Vec<f32>>),
            F32(Vec<Vec<f32>>),
            F64(Vec<Vec<f64>>),
        }

        let check_total_frames = |total_frames: &mut u64,
                                  max_frames: u64,
                                  packet_len: usize,
                                  path: &PathBuf|
         -> Result<(), PcmLoadError> {
            *total_frames += packet_len as u64;
            if *total_frames > max_frames {
                Err(PcmLoadError::FileTooLarge(path.clone()))
            } else {
                Ok(())
            }
        };

        // Decode the first packet to get the sample format.
        let mut first_packet = None;
        while let Ok(packet) = probed.format.next_packet() {
            // If the packet does not belong to the selected track, skip over it.
            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => match decoded {
                    AudioBufferRef::U8(d) => {
                        let mut decoded_channels = Vec::<Vec<u8>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_u8_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::U8(decoded_channels));
                        break;
                    }
                    AudioBufferRef::U16(d) => {
                        let mut decoded_channels = Vec::<Vec<u16>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_u16_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::U16(decoded_channels));
                        break;
                    }
                    AudioBufferRef::U24(d) => {
                        let mut decoded_channels = Vec::<Vec<[u8; 3]>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_u24_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::U24(decoded_channels));
                        break;
                    }
                    AudioBufferRef::U32(d) => {
                        let mut decoded_channels = Vec::<Vec<f32>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_u32_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::U32(decoded_channels));
                        break;
                    }
                    AudioBufferRef::S8(d) => {
                        let mut decoded_channels = Vec::<Vec<i8>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_i8_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::S8(decoded_channels));
                        break;
                    }
                    AudioBufferRef::S16(d) => {
                        let mut decoded_channels = Vec::<Vec<i16>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_i16_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::S16(decoded_channels));
                        break;
                    }
                    AudioBufferRef::S24(d) => {
                        let mut decoded_channels = Vec::<Vec<[u8; 3]>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_i24_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::S24(decoded_channels));
                        break;
                    }
                    AudioBufferRef::S32(d) => {
                        let mut decoded_channels = Vec::<Vec<f32>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_i32_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::S32(decoded_channels));
                        break;
                    }
                    AudioBufferRef::F32(d) => {
                        let mut decoded_channels = Vec::<Vec<f32>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_f32_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::F32(decoded_channels));
                        break;
                    }
                    AudioBufferRef::F64(d) => {
                        let mut decoded_channels = Vec::<Vec<f64>>::new();
                        for _ in 0..n_channels {
                            decoded_channels
                                .push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                        }

                        check_total_frames(&mut total_frames, max_frames, d.chan(0).len(), path)?;

                        decode::decode_f64_packet(&mut decoded_channels, d, n_channels);

                        first_packet = Some(FirstPacketType::F64(decoded_channels));
                        break;
                    }
                },
                Err(symphonia::core::errors::Error::DecodeError(err)) => {
                    // Decode errors are not fatal. Print the error message and try to decode the next
                    // packet as usual.
                    log::warn!("Symphonia decode warning: {}", err);
                }
                Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
            };
        }

        if first_packet.is_none() {
            return Err(PcmLoadError::UnexpectedErrorWhileDecoding((
                path.clone(),
                "no packet was found".into(),
            )));
        }

        let unexpected_format = |expected: &str| -> PcmLoadError {
            PcmLoadError::UnexpectedErrorWhileDecoding((
                path.clone(),
                format!(
                    "Symphonia returned a packet that was not the expected format of {}",
                    expected
                )
                .into(),
            ))
        };

        let decode_warning = |err: &str| {
            // Decode errors are not fatal. Print the error message and try to decode the next
            // packet as usual.
            log::warn!("Symphonia decode warning: {}", err);
        };

        let pcm_type = match first_packet.take().unwrap() {
            FirstPacketType::U8(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::U8(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_u8_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("u8")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::U8(decoded_channels)
            }
            FirstPacketType::U16(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::U16(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_u16_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("u16")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::U16(decoded_channels)
            }
            FirstPacketType::U24(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::U24(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_u24_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("u24")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::U24(decoded_channels)
            }
            FirstPacketType::U32(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::U32(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_u32_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("u32")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::F32(decoded_channels)
            }
            FirstPacketType::S8(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::S8(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_i8_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("i8")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::S8(decoded_channels)
            }
            FirstPacketType::S16(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::S16(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_i16_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("i16")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::S16(decoded_channels)
            }
            FirstPacketType::S24(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::S24(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_i24_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("i24")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::S24(decoded_channels)
            }
            FirstPacketType::S32(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::S32(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_i32_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("i32")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::F32(decoded_channels)
            }
            FirstPacketType::F32(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::F32(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_f32_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("f32")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::F32(decoded_channels)
            }
            FirstPacketType::F64(mut decoded_channels) => {
                while let Ok(packet) = probed.format.next_packet() {
                    // If the packet does not belong to the selected track, skip over it.
                    if packet.track_id() != track_id {
                        continue;
                    }

                    match decoder.decode(&packet) {
                        Ok(decoded) => match decoded {
                            AudioBufferRef::F64(d) => {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    path,
                                )?;

                                decode::decode_f64_packet(&mut decoded_channels, d, n_channels);
                            }
                            _ => return Err(unexpected_format("f64")),
                        },
                        Err(symphonia::core::errors::Error::DecodeError(err)) => {
                            decode_warning(err)
                        }
                        Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
                    }
                }

                PcmResourceType::F64(decoded_channels)
            }
        };

        let pcm = Shared::new(
            &self.coll_handle,
            PcmResource {
                pcm_type,
                sample_rate,
                channels: n_channels,
                len_frames: Frame(total_frames),
            },
        );

        self.loaded.insert(path.to_owned(), Shared::clone(&pcm));

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
                path,
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
        }
    }
}
