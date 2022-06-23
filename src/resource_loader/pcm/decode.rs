use std::path::PathBuf;

use symphonia::core::audio::{AudioBuffer, AudioBufferRef, SampleBuffer, Signal};
use symphonia::core::codecs::Decoder;
use symphonia::core::probe::ProbeResult;

use super::loader::PcmLoadError;
use super::AnyPcm;

fn decode_u8(
    decoder: &mut Box<dyn Decoder>,
    probed: &mut ProbeResult,
    max_frames: u64,
    num_channels: usize,
    num_frames: Option<u64>,
    track_id: u32,
    path: &PathBuf,
) -> Result<AnyPcm, PcmLoadError> {
    let mut total_frames = 0;

    let mut decoded_channels = Vec::<Vec<u8>>::new();
    for _ in 0..num_channels {
        decoded_channels.push(Vec::with_capacity(num_frames.unwrap_or(0) as usize));
    }

    while let Ok(packet) = probed.format.next_packet() {
        // If the packet does not belong to the selected track, skip over it.
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => match decoded {
                AudioBufferRef::U8(d) => {
                    total_frames += d.chan(0).len() as u64;
                    if total_frames > max_frames {
                        return Err(PcmLoadError::FileTooLarge(path.clone()));
                    }
                    for i in 0..num_channels {
                        decoded_channels[i].extend_from_slice(d.chan(i));
                    }
                }
                _ => return Err(PcmLoadError::UnexpectedErrorWhileDecoding((
                    path.clone(),
                    "Symphonia returned a decoded packet that was not in the expected format of u8"
                        .into(),
                ))),
            },
            Err(symphonia::core::errors::Error::DecodeError(err)) => {
                // Decode errors are not fatal. Print the error message and try to decode the next
                // packet as usual.
                log::warn!("Symphonia decode warning: {}", err);
            }
            Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
        }
    }

    Ok(decoded_channels)
}

fn decode_u16(
    decoder: &mut Box<dyn Decoder>,
    probed: &mut ProbeResult,
    max_frames: u64,
    num_channels: usize,
    num_frames: Option<u64>,
    track_id: u32,
    path: &PathBuf,
) -> Result<Vec<Vec<u16>>, PcmLoadError> {
    let mut total_frames = 0;

    let mut decoded_channels = Vec::<Vec<u16>>::new();
    for _ in 0..num_channels {
        decoded_channels.push(Vec::with_capacity(num_frames.unwrap_or(0) as usize));
    }

    while let Ok(packet) = probed.format.next_packet() {
        // If the packet does not belong to the selected track, skip over it.
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                match decoded {
                    AudioBufferRef::U16(d) => {
                        total_frames += d.chan(0).len() as u64;
                        if total_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(path.clone()));
                        }
                        for i in 0..num_channels {
                            decoded_channels[i].extend_from_slice(d.chan(i));
                        }
                    }
                    _ => {
                        return Err(PcmLoadError::UnexpectedErrorWhileDecoding((path.clone(), "Symphonia returned a decoded packet that was not in the expected format of u16".into())))
                    }
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(err)) => {
                // Decode errors are not fatal. Print the error message and try to decode the next
                // packet as usual.
                log::warn!("Symphonia decode warning: {}", err);
            }
            Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((path.clone(), e))),
        }
    }

    Ok(decoded_channels)
}
