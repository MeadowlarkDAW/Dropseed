mod decode;
pub mod loader;

pub use loader::{PcmKey, PcmLoadError, PcmLoader};
use meadowlark_core_types::{Frames, SampleRate};

pub struct PcmResource {
    pub pcm_type: PcmResourceType,
    pub sample_rate: SampleRate,
    pub channels: usize,
    pub len_frames: Frames,
}

/// The format of the raw PCM samples
///
/// Note that there is no option for U32/I32. This is because we want to use
/// float for everything anyway. We only store the other types to save memory.
pub enum PcmResourceType {
    U8(Vec<Vec<u8>>),
    U16(Vec<Vec<u16>>),
    U24(Vec<Vec<[u8; 3]>>),
    S8(Vec<Vec<i8>>),
    S16(Vec<Vec<i16>>),
    S24(Vec<Vec<[u8; 3]>>),
    F32(Vec<Vec<f32>>),
    F64(Vec<Vec<f64>>),
}

impl PcmResource {
    pub fn fill_channel_f32(
        &self,
        channel: usize,
        frame: isize,
        buf: &mut [f32],
    ) -> Result<(), ()> {
        // TODO: Manual SIMD

        if channel >= self.channels {
            buf.fill(0.0);
            return Err(());
        }

        let len_frames = self.len_frames.0 as usize;
        let buf_len = buf.len();

        let (buf_range, pcm_range) =
            if frame >= len_frames as isize || frame + buf_len as isize <= 0 {
                // out of range, fill buffer with zeros
                buf.fill(0.0);
                return Ok(());
            } else if frame < 0 {
                let skip_frames = (0 - frame) as usize;

                // clear the out-of-range part
                buf[0..skip_frames].fill(0.0);

                let new_buf_len = buf_len - skip_frames;

                if new_buf_len <= len_frames {
                    ((skip_frames..buf_len), (0..new_buf_len))
                } else {
                    let copy_frames = len_frames - new_buf_len;

                    // clear the out-of-range part
                    buf[skip_frames + copy_frames..buf_len].fill(0.0);

                    ((skip_frames..skip_frames + copy_frames), (0..copy_frames))
                }
            } else if frame as usize + buf_len <= len_frames {
                ((0..buf_len), (frame as usize..frame as usize + buf_len))
            } else {
                let copy_frames = len_frames - frame as usize;

                // clear the out-of-range part
                buf[copy_frames..buf_len].fill(0.0);

                ((0..copy_frames), (frame as usize..len_frames))
            };

        match &self.pcm_type {
            PcmResourceType::U8(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = ((f32::from(*p)) * (2.0 / std::u8::MAX as f32)) - 1.0;
                }
            }
            PcmResourceType::U16(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = ((f32::from(*p)) * (2.0 / std::u16::MAX as f32)) - 1.0;
                }
            }
            PcmResourceType::U24(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                if cfg!(target_endian = "little") {
                    for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                        // In little-endian the MSB is the last byte.
                        let bytes = [p[0], p[1], p[2], 0];

                        let val = u32::from_ne_bytes(bytes);

                        *b = ((f64::from(val) * (2.0 / 16_777_215.0)) - 1.0) as f32;
                    }
                } else {
                    for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                        // In big-endian the MSB is the first byte.
                        let bytes = [0, p[0], p[1], p[2]];

                        let val = u32::from_ne_bytes(bytes);

                        *b = ((f64::from(val) * (2.0 / 16_777_215.0)) - 1.0) as f32;
                    }
                }
            }
            PcmResourceType::S8(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = f32::from(*p) / std::i8::MAX as f32;
                }
            }
            PcmResourceType::S16(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = f32::from(*p) / std::i16::MAX as f32;
                }
            }
            PcmResourceType::S24(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                if cfg!(target_endian = "little") {
                    for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                        // In little-endian the MSB is the last byte.
                        let bytes = [p[0], p[1], p[2], 0];

                        let val = i32::from_ne_bytes(bytes);

                        *b = (f64::from(val) / 8_388_607.0) as f32;
                    }
                } else {
                    for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                        // In big-endian the MSB is the first byte.
                        let bytes = [0, p[0], p[1], p[2]];

                        let val = i32::from_ne_bytes(bytes);

                        *b = (f64::from(val) / 8_388_607.0) as f32;
                    }
                }
            }
            PcmResourceType::F32(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                buf_part.copy_from_slice(pcm_part);
            }
            PcmResourceType::F64(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = *p as f32;
                }
            }
        }

        Ok(())
    }

    pub fn fill_stereo_f32(&self, frame: isize, buf_l: &mut [f32], buf_r: &mut [f32]) {
        // TODO: Manual SIMD

        assert_eq!(buf_l.len(), buf_r.len());

        if self.channels == 1 {
            self.fill_channel_f32(0, frame, buf_l).unwrap();
            buf_r.copy_from_slice(buf_l);
            return;
        }

        let len_frames = self.len_frames.0 as usize;
        let buf_l_len = buf_l.len();

        let (buf_range, pcm_range) =
            if frame >= len_frames as isize || frame + buf_l_len as isize <= 0 {
                // out of range, fill buffer with zeros
                buf_l.fill(0.0);
                buf_r.fill(0.0);
                return;
            } else if frame < 0 {
                let skip_frames = (0 - frame) as usize;

                // clear the out-of-range part
                buf_l[0..skip_frames].fill(0.0);
                buf_r[0..skip_frames].fill(0.0);

                let new_buf_len = buf_l_len - skip_frames;

                if new_buf_len <= len_frames {
                    ((skip_frames..buf_l_len), (0..new_buf_len))
                } else {
                    let copy_frames = len_frames - new_buf_len;

                    // clear the out-of-range part
                    buf_l[skip_frames + copy_frames..buf_l_len].fill(0.0);
                    buf_r[skip_frames + copy_frames..buf_l_len].fill(0.0);

                    ((skip_frames..skip_frames + copy_frames), (0..copy_frames))
                }
            } else if frame as usize + buf_l_len <= len_frames {
                ((0..buf_l_len), (frame as usize..frame as usize + buf_l_len))
            } else {
                let copy_frames = len_frames - frame as usize;

                // clear the out-of-range part
                buf_l[copy_frames..buf_l_len].fill(0.0);
                buf_r[copy_frames..buf_l_len].fill(0.0);

                ((0..copy_frames), (frame as usize..len_frames))
            };

        match &self.pcm_type {
            PcmResourceType::U8(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] =
                        ((f32::from(pcm_l_part[i])) * (2.0 / std::u8::MAX as f32)) - 1.0;
                    buf_r_part[i] =
                        ((f32::from(pcm_r_part[i])) * (2.0 / std::u8::MAX as f32)) - 1.0;
                }
            }
            PcmResourceType::U16(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] =
                        ((f32::from(pcm_l_part[i])) * (2.0 / std::u16::MAX as f32)) - 1.0;
                    buf_r_part[i] =
                        ((f32::from(pcm_r_part[i])) * (2.0 / std::u16::MAX as f32)) - 1.0;
                }
            }
            PcmResourceType::U24(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                if cfg!(target_endian = "little") {
                    for i in 0..buf_l_part.len() {
                        // In little-endian the MSB is the last byte.
                        let bytes_l = [pcm_l_part[i][0], pcm_l_part[i][1], pcm_l_part[i][2], 0];
                        let bytes_r = [pcm_r_part[i][0], pcm_r_part[i][1], pcm_r_part[i][2], 0];

                        let val_l = u32::from_ne_bytes(bytes_l);
                        let val_r = u32::from_ne_bytes(bytes_r);

                        buf_l_part[i] = ((f64::from(val_l) * (2.0 / 16_777_215.0)) - 1.0) as f32;
                        buf_r_part[i] = ((f64::from(val_r) * (2.0 / 16_777_215.0)) - 1.0) as f32;
                    }
                } else {
                    for i in 0..buf_l_part.len() {
                        // In big-endian the MSB is the first byte.
                        let bytes_l = [0, pcm_l_part[i][0], pcm_l_part[i][1], pcm_l_part[i][2]];
                        let bytes_r = [0, pcm_r_part[i][0], pcm_r_part[i][1], pcm_r_part[i][2]];

                        let val_l = u32::from_ne_bytes(bytes_l);
                        let val_r = u32::from_ne_bytes(bytes_r);

                        buf_l_part[i] = ((f64::from(val_l) * (2.0 / 16_777_215.0)) - 1.0) as f32;
                        buf_r_part[i] = ((f64::from(val_r) * (2.0 / 16_777_215.0)) - 1.0) as f32;
                    }
                }
            }
            PcmResourceType::S8(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = f32::from(pcm_l_part[i]) / std::i8::MAX as f32;
                    buf_r_part[i] = f32::from(pcm_r_part[i]) / std::i8::MAX as f32;
                }
            }
            PcmResourceType::S16(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = f32::from(pcm_l_part[i]) / std::i16::MAX as f32;
                    buf_r_part[i] = f32::from(pcm_r_part[i]) / std::i16::MAX as f32;
                }
            }
            PcmResourceType::S24(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                if cfg!(target_endian = "little") {
                    for i in 0..buf_l_part.len() {
                        // In little-endian the MSB is the last byte.
                        let bytes_l = [pcm_l_part[i][0], pcm_l_part[i][1], pcm_l_part[i][2], 0];
                        let bytes_r = [pcm_r_part[i][0], pcm_r_part[i][1], pcm_r_part[i][2], 0];

                        let val_l = i32::from_ne_bytes(bytes_l);
                        let val_r = i32::from_ne_bytes(bytes_r);

                        buf_l_part[i] = (f64::from(val_l) / 8_388_607.0) as f32;
                        buf_r_part[i] = (f64::from(val_r) / 8_388_607.0) as f32;
                    }
                } else {
                    for i in 0..buf_l_part.len() {
                        // In big-endian the MSB is the first byte.
                        let bytes_l = [0, pcm_l_part[i][0], pcm_l_part[i][1], pcm_l_part[i][2]];
                        let bytes_r = [0, pcm_r_part[i][0], pcm_r_part[i][1], pcm_r_part[i][2]];

                        let val_l = i32::from_ne_bytes(bytes_l);
                        let val_r = i32::from_ne_bytes(bytes_r);

                        buf_l_part[i] = (f64::from(val_l) / 8_388_607.0) as f32;
                        buf_r_part[i] = (f64::from(val_r) / 8_388_607.0) as f32;
                    }
                }
            }
            PcmResourceType::F32(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                buf_l_part.copy_from_slice(pcm_l_part);
                buf_r_part.copy_from_slice(pcm_r_part);
            }
            PcmResourceType::F64(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[0][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = pcm_l_part[i] as f32;
                    buf_r_part[i] = pcm_r_part[i] as f32;
                }
            }
        }
    }
}
