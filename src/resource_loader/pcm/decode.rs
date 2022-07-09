use std::borrow::Cow;
use std::path::PathBuf;

use meadowlark_core_types::{Frames, SampleRate};
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::audio::{AudioBuffer, Signal};
use symphonia::core::codecs::{CodecRegistry, DecoderOptions};
use symphonia::core::probe::ProbeResult;
use symphonia::core::sample::{i24, u24};

use super::loader::MAX_FILE_BYTES;
use super::{PcmKey, PcmLoadError, PcmResource, PcmResourceType};

/*
pub(crate) fn decode_f32_resampled(
    probed: &mut ProbeResult,
    key: &PcmKey,
    codec_registry: &CodecRegistry,
    pcm_sample_rate: SampleRate,
    target_sample_rate: SampleRate,
) -> Result<PcmResource, PcmLoadError> {
    // Get the default track in the audio stream.
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| PcmLoadError::NoTrackFound(key.path.clone()))?;
    let track_id = track.id;

    // Get info.
    let n_channels = track
        .codec_params
        .channels
        .ok_or_else(|| PcmLoadError::NoChannelsFound(key.path.clone()))?
        .count();

    if n_channels == 0 {
        return Err(PcmLoadError::NoChannelsFound(key.path.clone()));
    }

    let n_frames = track.codec_params.n_frames;

    let decode_opts: DecoderOptions = Default::default();

    // Create a decoder for the track.
    let mut decoder = codec_registry
        .make(&track.codec_params, &decode_opts)
        .map_err(|e| PcmLoadError::CouldNotCreateDecoder((key.path.clone(), e)))?;

    let mut total_frames = 0;
    let max_frames = MAX_FILE_BYTES / (4 * n_channels as u64);

    if let Some(n_frames) = n_frames {
        if n_frames > max_frames {
            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
        }
    }

    let mut interleaved_buffer: Vec<f32> = Vec::with_capacity(n_frames.unwrap_or(44100) as usize * n_channels);
    let mut sample_buf = None;

    let decode_warning = |err: &str| {
        // Decode errors are not fatal. Print the error message and try to decode the next
        // packet as usual.
        log::warn!("Symphonia decode warning: {}", err);
    };

    while let Ok(packet) = probed.format.next_packet() {
        // If the packet does not belong to the selected track, skip over it.
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                // If this is the *first* decoded packet, create a sample buffer matching the
                // decoded audio buffer format.
                if sample_buf.is_none() {
                    // Get the audio buffer specification.
                    let spec = *decoded.spec();

                    // Get the capacity of the decoded buffer. Note: This is capacity, not length!
                    let duration = decoded.capacity() as u64;

                    // Create the f32 sample buffer.
                    sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                }

                if n_frames.is_none() {
                    total_frames += decoded.frames() as u64;
                    if total_frames > max_frames {
                        return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                    }
                }

                let s = sample_buf.as_mut().unwrap();

                // Copy the decoded audio buffer into the sample buffer in an interleaved format.
                s.copy_interleaved_ref(decoded);

                interleaved_buffer.extend_from_slice(s.samples());
            },
            Err(symphonia::core::errors::Error::DecodeError(err)) => {
                decode_warning(err)
            }
            Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
        }
    }

    let converter_type = key.quality.as_converter_type();

    let resampled = match samplerate::convert(pcm_sample_rate.as_u32(), target_sample_rate.as_u32(), n_channels, converter_type, &interleaved_buffer) {
        Ok(r) => r,
        Err(e) => {
            return Err(PcmLoadError::ErrorWhileResampling((key.path.clone(), e)));
        }
    };

    let total_frames = resampled.len() / n_channels;

    // TODO: Optimize this deinterleaving loop?
    let mut channels: Vec<Vec<f32>> = Vec::with_capacity(n_channels);
    for ch_i in 0..n_channels {
        let mut ch_buf: Vec<f32> = Vec::with_capacity(total_frames);

        for i in 0..total_frames {
            ch_buf.push(resampled[(i * n_channels) + ch_i]);
        }

        channels.push(ch_buf);
    }

    Ok(
        PcmResource {
            pcm_type: PcmResourceType::F32(channels),
            sample_rate: target_sample_rate,
            channels: n_channels,
            len_frames: Frames(total_frames as u64),
        }
    )
}
*/

pub(crate) fn decode_native_bitdepth(
    probed: &mut ProbeResult,
    key: &PcmKey,
    codec_registry: &CodecRegistry,
    sample_rate: SampleRate,
) -> Result<PcmResource, PcmLoadError> {
    // Get the default track in the audio stream.
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| PcmLoadError::NoTrackFound(key.path.clone()))?;
    let track_id = track.id;

    // Get info.
    let n_channels = track
        .codec_params
        .channels
        .ok_or_else(|| PcmLoadError::NoChannelsFound(key.path.clone()))?
        .count();

    if n_channels == 0 {
        return Err(PcmLoadError::NoChannelsFound(key.path.clone()));
    }

    let n_frames = track.codec_params.n_frames;

    let decode_opts: DecoderOptions = Default::default();

    // Create a decoder for the track.
    let mut decoder = codec_registry
        .make(&track.codec_params, &decode_opts)
        .map_err(|e| PcmLoadError::CouldNotCreateDecoder((key.path.clone(), e)))?;

    let mut max_frames = 0;
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
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / n_channels as u64;
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_u8_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::U8(decoded_channels));
                    break;
                }
                AudioBufferRef::U16(d) => {
                    let mut decoded_channels = Vec::<Vec<u16>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (2 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_u16_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::U16(decoded_channels));
                    break;
                }
                AudioBufferRef::U24(d) => {
                    let mut decoded_channels = Vec::<Vec<[u8; 3]>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (3 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_u24_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::U24(decoded_channels));
                    break;
                }
                AudioBufferRef::U32(d) => {
                    let mut decoded_channels = Vec::<Vec<f32>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (4 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_u32_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::U32(decoded_channels));
                    break;
                }
                AudioBufferRef::S8(d) => {
                    let mut decoded_channels = Vec::<Vec<i8>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / n_channels as u64;
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_i8_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::S8(decoded_channels));
                    break;
                }
                AudioBufferRef::S16(d) => {
                    let mut decoded_channels = Vec::<Vec<i16>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (2 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_i16_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::S16(decoded_channels));
                    break;
                }
                AudioBufferRef::S24(d) => {
                    let mut decoded_channels = Vec::<Vec<[u8; 3]>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (3 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_i24_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::S24(decoded_channels));
                    break;
                }
                AudioBufferRef::S32(d) => {
                    let mut decoded_channels = Vec::<Vec<f32>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (4 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_i32_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::S32(decoded_channels));
                    break;
                }
                AudioBufferRef::F32(d) => {
                    let mut decoded_channels = Vec::<Vec<f32>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (4 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_f32_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::F32(decoded_channels));
                    break;
                }
                AudioBufferRef::F64(d) => {
                    let mut decoded_channels = Vec::<Vec<f64>>::new();
                    for _ in 0..n_channels {
                        decoded_channels.push(Vec::with_capacity(n_frames.unwrap_or(0) as usize));
                    }

                    max_frames = MAX_FILE_BYTES / (8 * n_channels as u64);
                    if let Some(n_frames) = n_frames {
                        if n_frames > max_frames {
                            return Err(PcmLoadError::FileTooLarge(key.path.clone()));
                        }
                    } else {
                        check_total_frames(
                            &mut total_frames,
                            max_frames,
                            d.chan(0).len(),
                            &key.path,
                        )?;
                    }

                    decode_f64_packet(&mut decoded_channels, d, n_channels);

                    first_packet = Some(FirstPacketType::F64(decoded_channels));
                    break;
                }
            },
            Err(symphonia::core::errors::Error::DecodeError(err)) => {
                // Decode errors are not fatal. Print the error message and try to decode the next
                // packet as usual.
                log::warn!("Symphonia decode warning: {}", err);
            }
            Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
        };
    }

    if first_packet.is_none() {
        return Err(PcmLoadError::UnexpectedErrorWhileDecoding((
            key.path.clone(),
            "no packet was found".into(),
        )));
    }

    let unexpected_format = |expected: &str| -> PcmLoadError {
        PcmLoadError::UnexpectedErrorWhileDecoding((
            key.path.clone(),
            format!("Symphonia returned a packet that was not the expected format of {}", expected)
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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_u8_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("u8")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_u16_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("u16")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_u24_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("u24")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_u32_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("u32")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_i8_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("i8")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_i16_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("i16")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_i24_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("i24")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_i32_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("i32")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_f32_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("f32")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

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
                            if n_frames.is_none() {
                                check_total_frames(
                                    &mut total_frames,
                                    max_frames,
                                    d.chan(0).len(),
                                    &key.path,
                                )?;
                            }

                            decode_f64_packet(&mut decoded_channels, d, n_channels);
                        }
                        _ => return Err(unexpected_format("f64")),
                    },
                    Err(symphonia::core::errors::Error::DecodeError(err)) => decode_warning(err),
                    Err(e) => return Err(PcmLoadError::ErrorWhileDecoding((key.path.clone(), e))),
                }
            }

            total_frames = decoded_channels[0].len() as u64;

            PcmResourceType::F64(decoded_channels)
        }
    };

    Ok(PcmResource {
        pcm_type,
        sample_rate,
        channels: n_channels,
        len_frames: Frames(total_frames),
    })
}

#[inline]
fn decode_u8_packet(
    decoded_channels: &mut Vec<Vec<u8>>,
    packet: Cow<AudioBuffer<u8>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
fn decode_u16_packet(
    decoded_channels: &mut Vec<Vec<u16>>,
    packet: Cow<AudioBuffer<u16>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
fn decode_u24_packet(
    decoded_channels: &mut Vec<Vec<[u8; 3]>>,
    packet: Cow<AudioBuffer<u24>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        for s in packet.chan(i).iter() {
            decoded_channels[i].push(s.to_ne_bytes());
        }
    }
}

#[inline]
fn decode_u32_packet(
    decoded_channels: &mut Vec<Vec<f32>>,
    packet: Cow<AudioBuffer<u32>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        for s in packet.chan(i).iter() {
            let s_f32 = ((f64::from(*s) * (2.0 / std::u32::MAX as f64)) - 1.0) as f32;

            decoded_channels[i].push(s_f32);
        }
    }
}

#[inline]
fn decode_i8_packet(
    decoded_channels: &mut Vec<Vec<i8>>,
    packet: Cow<AudioBuffer<i8>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
fn decode_i16_packet(
    decoded_channels: &mut Vec<Vec<i16>>,
    packet: Cow<AudioBuffer<i16>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
fn decode_i24_packet(
    decoded_channels: &mut Vec<Vec<[u8; 3]>>,
    packet: Cow<AudioBuffer<i24>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        for s in packet.chan(i).iter() {
            decoded_channels[i].push(s.to_ne_bytes());
        }
    }
}

#[inline]
fn decode_i32_packet(
    decoded_channels: &mut Vec<Vec<f32>>,
    packet: Cow<AudioBuffer<i32>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        for s in packet.chan(i).iter() {
            let s_f32 = (f64::from(*s) / std::i32::MAX as f64) as f32;

            decoded_channels[i].push(s_f32);
        }
    }
}

#[inline]
fn decode_f32_packet(
    decoded_channels: &mut Vec<Vec<f32>>,
    packet: Cow<AudioBuffer<f32>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
fn decode_f64_packet(
    decoded_channels: &mut Vec<Vec<f64>>,
    packet: Cow<AudioBuffer<f64>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}
