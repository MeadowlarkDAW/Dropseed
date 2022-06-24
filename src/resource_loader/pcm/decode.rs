use std::borrow::Cow;

use symphonia::core::audio::{AudioBuffer, Signal};
use symphonia::core::sample::{i24, u24};

#[inline]
pub(super) fn decode_u8_packet(
    decoded_channels: &mut Vec<Vec<u8>>,
    packet: Cow<AudioBuffer<u8>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
pub(super) fn decode_u16_packet(
    decoded_channels: &mut Vec<Vec<u16>>,
    packet: Cow<AudioBuffer<u16>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
pub(super) fn decode_u24_packet(
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
pub(super) fn decode_u32_packet(
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
pub(super) fn decode_i8_packet(
    decoded_channels: &mut Vec<Vec<i8>>,
    packet: Cow<AudioBuffer<i8>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
pub(super) fn decode_i16_packet(
    decoded_channels: &mut Vec<Vec<i16>>,
    packet: Cow<AudioBuffer<i16>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
pub(super) fn decode_i24_packet(
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
pub(super) fn decode_i32_packet(
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
pub(super) fn decode_f32_packet(
    decoded_channels: &mut Vec<Vec<f32>>,
    packet: Cow<AudioBuffer<f32>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}

#[inline]
pub(super) fn decode_f64_packet(
    decoded_channels: &mut Vec<Vec<f64>>,
    packet: Cow<AudioBuffer<f64>>,
    num_channels: usize,
) {
    for i in 0..num_channels {
        decoded_channels[i].extend_from_slice(packet.chan(i));
    }
}
