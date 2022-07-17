use super::convert;

/// A resource of raw PCM samples stored in RAM. This struct stores samples
/// in their native sample format when possible to save memory.
///
/// Note that PCM resources are immutable by design.
pub struct PcmRAM {
    pcm_type: PcmRAMType,
    sample_rate: u32,
    channels: usize,
    len_frames: u64,
}

/// The format of the raw PCM samples store in RAM.
///
/// Note that there is no option for U32/I32. This is because we want to use
/// float for everything anyway. We only store the other types to save memory.
pub enum PcmRAMType {
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

impl PcmRAM {
    pub fn new(pcm_type: PcmRAMType, sample_rate: u32) -> Self {
        let (channels, len_frames) = match &pcm_type {
            PcmRAMType::U8(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::U16(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::U24(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::S8(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::S16(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::S24(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::F32(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
            PcmRAMType::F64(b) => {
                let len = b[0].len();

                for ch in b.iter().skip(1) {
                    assert_eq!(ch.len(), len);
                }

                (b.len(), len)
            }
        };

        Self { pcm_type, sample_rate, channels, len_frames: len_frames as u64 }
    }

    /// The number of channels in this resource.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The length of this resource in frames (length of a single channel in
    /// samples).
    pub fn len_frames(&self) -> u64 {
        self.len_frames
    }

    /// The sample rate of this resource in samples per second.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get(&self) -> &PcmRAMType {
        &self.pcm_type
    }

    /// Fill the buffer with samples from the given `channel`, starting from the
    /// given `frame`. Portions that are out-of-bounds will be filled with zeros.
    ///
    /// The will return an error if the given channel does not exist.
    pub fn fill_channel_f32(
        &self,
        channel: usize,
        frame: isize,
        buf: &mut [f32],
    ) -> Result<(), ()> {
        if channel >= self.channels {
            buf.fill(0.0);
            return Err(());
        }

        let len_frames = self.len_frames as usize;
        let buf_len = buf.len();

        let (buf_start, pcm_start, len) =
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
                    (skip_frames, 0, new_buf_len)
                } else {
                    let copy_frames = len_frames - new_buf_len;

                    // clear the out-of-range part
                    buf[skip_frames + copy_frames..buf_len].fill(0.0);

                    (skip_frames, 0, copy_frames)
                }
            } else if frame as usize + buf_len <= len_frames {
                (0, frame as usize, buf_len)
            } else {
                let copy_frames = len_frames - frame as usize;

                // clear the out-of-range part
                buf[copy_frames..buf_len].fill(0.0);

                (0, frame as usize, copy_frames)
            };

        debug_assert!(buf_start + len <= buf_len);

        match &self.pcm_type {
            PcmRAMType::U8(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, s) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_u8_to_f32(*s);
                }
            }
            PcmRAMType::U16(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_u16_to_f32(*p);
                }
            }
            PcmRAMType::U24(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_u24_to_f32_ne(*p);
                }
            }
            PcmRAMType::S8(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_s8_to_f32(*p);
                }
            }
            PcmRAMType::S16(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_s16_to_f32(*p);
                }
            }
            PcmRAMType::S24(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = convert::pcm_s24_to_f32_ne(*p);
                }
            }
            PcmRAMType::F32(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                buf_part.copy_from_slice(pcm_part);
            }
            PcmRAMType::F64(pcm) => {
                debug_assert!(pcm_start + len <= pcm[channel].len());

                let buf_part = &mut buf[buf_start..buf_start + len];
                let pcm_part = &pcm[channel][pcm_start..pcm_start + len];

                for (b, p) in buf_part.iter_mut().zip(pcm_part.iter()) {
                    *b = *p as f32;
                }
            }
        }

        Ok(())
    }

    /// Fill the stereo buffer with samples, starting from the given `frame`.
    /// Portions that are out-of-bounds will be filled with zeros.
    ///
    /// If this resource has only one channel, then both channels will be
    /// filled with the same data.
    pub fn fill_stereo_f32(&self, frame: isize, buf_l: &mut [f32], buf_r: &mut [f32]) {
        debug_assert_eq!(buf_l.len(), buf_r.len());

        if self.channels == 1 {
            self.fill_channel_f32(0, frame, buf_l).unwrap();
            buf_r.copy_from_slice(buf_l);
            return;
        }

        let len_frames = self.len_frames as usize;
        let buf_len = buf_l.len();

        let (buf_start, pcm_start, len) =
            if frame >= len_frames as isize || frame + buf_len as isize <= 0 {
                // out of range, fill buffer with zeros
                buf_l.fill(0.0);
                buf_r.fill(0.0);
                return;
            } else if frame < 0 {
                let skip_frames = (0 - frame) as usize;

                // clear the out-of-range part
                buf_l[0..skip_frames].fill(0.0);
                buf_r[0..skip_frames].fill(0.0);

                let new_buf_len = buf_len - skip_frames;

                if new_buf_len <= len_frames {
                    (skip_frames, 0, new_buf_len)
                } else {
                    let copy_frames = len_frames - new_buf_len;

                    // clear the out-of-range part
                    buf_l[skip_frames + copy_frames..buf_len].fill(0.0);
                    buf_r[skip_frames + copy_frames..buf_len].fill(0.0);

                    (skip_frames, 0, copy_frames)
                }
            } else if frame as usize + buf_len <= len_frames {
                (0, frame as usize, buf_len)
            } else {
                let copy_frames = len_frames - frame as usize;

                // clear the out-of-range part
                buf_l[copy_frames..buf_len].fill(0.0);
                buf_r[copy_frames..buf_len].fill(0.0);

                (0, frame as usize, copy_frames)
            };

        debug_assert!(buf_start + len <= buf_len);

        match &self.pcm_type {
            PcmRAMType::U8(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_u8_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_u8_to_f32(pcm_r_part[i]);
                }
            }
            PcmRAMType::U16(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_u16_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_u16_to_f32(pcm_r_part[i]);
                }
            }
            PcmRAMType::U24(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_u24_to_f32_ne(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_u24_to_f32_ne(pcm_r_part[i]);
                }
            }
            PcmRAMType::S8(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_s8_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_s8_to_f32(pcm_r_part[i]);
                }
            }
            PcmRAMType::S16(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_s16_to_f32(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_s16_to_f32(pcm_r_part[i]);
                }
            }
            PcmRAMType::S24(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = convert::pcm_s24_to_f32_ne(pcm_l_part[i]);
                    buf_r_part[i] = convert::pcm_s24_to_f32_ne(pcm_r_part[i]);
                }
            }
            PcmRAMType::F32(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                buf_l_part.copy_from_slice(pcm_l_part);
                buf_r_part.copy_from_slice(pcm_r_part);
            }
            PcmRAMType::F64(pcm) => {
                debug_assert!(pcm_start + len <= pcm[0].len());
                debug_assert!(pcm_start + len <= pcm[1].len());

                let buf_l_part = &mut buf_l[buf_start..buf_start + len];
                let buf_r_part = &mut buf_r[buf_start..buf_start + len];
                let pcm_l_part = &pcm[0][pcm_start..pcm_start + len];
                let pcm_r_part = &pcm[1][pcm_start..pcm_start + len];

                for i in 0..buf_l_part.len() {
                    buf_l_part[i] = pcm_l_part[i] as f32;
                    buf_r_part[i] = pcm_r_part[i] as f32;
                }
            }
        }
    }

    /// Consume this resource and return the raw samples.
    pub fn to_raw(self) -> PcmRAMType {
        self.pcm_type
    }
}
