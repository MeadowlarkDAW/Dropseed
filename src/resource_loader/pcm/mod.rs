pub mod convert;
mod decode;
pub mod loader;

pub use loader::{PcmKey, PcmLoadError, PcmLoader, ResampleQuality};
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
    /// The endianness of the samples must be the native endianness of the
    /// target platform.
    U24(Vec<Vec<[u8; 3]>>),
    S8(Vec<Vec<i8>>),
    S16(Vec<Vec<i16>>),
    /// The endianness of the samples must be the native endianness of the
    /// target platform.
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

                for (b, s) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_u8_to_f32(*s);
                }
            }
            PcmResourceType::U16(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_u16_to_f32(*p);
                }
            }
            PcmResourceType::U24(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_u24_to_f32_ne(*p);
                }
            }
            PcmResourceType::S8(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_s8_to_f32(*p);
                }
            }
            PcmResourceType::S16(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_s16_to_f32(*p);
                }
            }
            PcmResourceType::S24(pcm) => {
                let buf_part = &mut buf[buf_range];
                let pcm_part = &pcm[channel][pcm_range];

                assert_eq!(buf_part.len(), pcm_part.len());

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_s24_to_f32_ne(*p);
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
                let pcm_r_part = &pcm[1][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_u8_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_u8_to_f32(pcm_r_part[i]);
                }
            }
            PcmResourceType::U16(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[1][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_u16_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_u16_to_f32(pcm_r_part[i]);
                }
            }
            PcmResourceType::U24(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[1][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_u24_to_f32_ne(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_u24_to_f32_ne(pcm_r_part[i]);
                }
            }
            PcmResourceType::S8(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[1][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_s8_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_s8_to_f32(pcm_r_part[i]);
                }
            }
            PcmResourceType::S16(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[1][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_s16_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_s16_to_f32(pcm_r_part[i]);
                }
            }
            PcmResourceType::S24(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[1][pcm_range];

                assert_eq!(buf_l_part.len(), pcm_l_part.len());
                assert_eq!(buf_r_part.len(), pcm_r_part.len());
                assert_eq!(buf_l_part.len(), buf_r_part.len());

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_s24_to_f32_ne(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_s24_to_f32_ne(pcm_r_part[i]);
                }
            }
            PcmResourceType::F32(pcm) => {
                let buf_l_part = &mut buf_l[buf_range.clone()];
                let buf_r_part = &mut buf_r[buf_range];
                let pcm_l_part = &pcm[0][pcm_range.clone()];
                let pcm_r_part = &pcm[1][pcm_range];

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
                let pcm_r_part = &pcm[1][pcm_range];

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
