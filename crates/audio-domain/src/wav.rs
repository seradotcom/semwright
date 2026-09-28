//! Bounded streaming RIFF/WAVE PCM and IEEE-float decoder. No codec execution.
//! RF64, compressed formats and ambiguous multiple data chunks are explicit unsupported cases.
use crate::{
    Error, Result,
    signal_analysis::{PcmAnalyzer, SignalStatistics},
    time::{MAX_SAMPLE_FRAME, SampleRate},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaveInfo {
    pub sample_rate: SampleRate,
    pub channels: u16,
    pub container_bits: u16,
    pub valid_bits: u16,
    pub ieee_float: bool,
    pub channel_mask: Option<u32>,
    pub frames: u64,
}
pub struct WaveReader<R> {
    reader: R,
    info: WaveInfo,
    data_start: u64,
    data_bytes: u64,
    frames_read: u64,
}
fn u16le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn io_error(_: std::io::Error) -> Error {
    Error::invalid("Truncated or unreadable RIFF/WAVE input")
}
impl<R: Read + Seek> WaveReader<R> {
    pub fn open(mut reader: R, byte_budget: u64) -> Result<Self> {
        let bytes = reader.seek(SeekFrom::End(0)).map_err(io_error)?;
        if bytes < 44 || bytes > byte_budget {
            return Err(Error::limit("WAVE file exceeds the input size contract"));
        }
        reader.seek(SeekFrom::Start(0)).map_err(io_error)?;
        let mut header = [0; 12];
        reader.read_exact(&mut header).map_err(io_error)?;
        if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
            return Err(Error::unsupported("Expected little-endian RIFF/WAVE"));
        }
        let end = u64::from(u32le(&header[4..8])) + 8;
        if end != bytes {
            return Err(Error::invalid(
                "RIFF declared size differs from actual bytes",
            ));
        }
        let mut info = None;
        let mut data = None;
        let mut cursor = 12;
        let mut chunks = 0;
        while cursor < end {
            chunks += 1;
            if chunks > 4096 || end - cursor < 8 {
                return Err(Error::invalid("Invalid RIFF chunk envelope"));
            }
            let mut chunk = [0; 8];
            reader.read_exact(&mut chunk).map_err(io_error)?;
            let length = u64::from(u32le(&chunk[4..]));
            let next = cursor + 8 + length + length % 2;
            if next > end {
                return Err(Error::invalid("RIFF chunk exceeds declared file"));
            }
            if &chunk[..4] == b"fmt " {
                if info.is_some() || !(16..=4096).contains(&length) {
                    return Err(Error::invalid("Ambiguous or invalid WAVE format chunk"));
                }
                let mut fmt = vec![0; length as usize];
                reader.read_exact(&mut fmt).map_err(io_error)?;
                let mut tag = u16le(&fmt);
                let channels = u16le(&fmt[2..]);
                let rate = SampleRate::new(u32le(&fmt[4..8]))?;
                let bytes_per_second = u32le(&fmt[8..12]);
                let align = u16le(&fmt[12..14]);
                let bits = u16le(&fmt[14..16]);
                let mut valid = bits;
                let mut mask = None;
                if tag == 0xfffe {
                    if fmt.len() < 40 || u16le(&fmt[16..18]) < 22 {
                        return Err(Error::invalid("Short extensible WAVE format"));
                    }
                    valid = u16le(&fmt[18..20]);
                    mask = Some(u32le(&fmt[20..24]));
                    // PCM/IEEE_FLOAT subtype GUID, including the complete namespace tail.
                    if fmt[26..40] != [0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113] {
                        return Err(Error::unsupported("Unknown WAVE subtype GUID"));
                    }
                    tag = u16le(&fmt[24..26]);
                    if mask.is_some_and(|m| m != 0 && m.count_ones() != u32::from(channels)) {
                        return Err(Error::invalid(
                            "Channel mask does not match WAVE channel count",
                        ));
                    }
                }
                if !(1..=64).contains(&channels)
                    || !matches!(bits, 8 | 16 | 24 | 32 | 64)
                    || valid == 0
                    || valid > bits
                    || (tag == 1 && bits == 64)
                    || (tag == 3 && (!matches!(bits, 32 | 64) || valid != bits))
                    || !matches!(tag, 1 | 3)
                {
                    return Err(Error::unsupported(
                        "Unsupported PCM format or inconsistent sample container",
                    ));
                }
                if align != channels * (bits / 8)
                    || u64::from(bytes_per_second) != u64::from(rate.0) * u64::from(align)
                {
                    return Err(Error::invalid("Invalid WAVE block alignment or byte rate"));
                }
                info = Some(WaveInfo {
                    sample_rate: rate,
                    channels,
                    container_bits: bits,
                    valid_bits: valid,
                    ieee_float: tag == 3,
                    channel_mask: mask,
                    frames: 0,
                });
            } else if &chunk[..4] == b"data" {
                if data.is_some() {
                    return Err(Error::unsupported(
                        "Multiple WAVE data chunks require an explicit import adapter",
                    ));
                }
                data = Some((cursor + 8, length));
            }
            cursor = next;
            reader.seek(SeekFrom::Start(next)).map_err(io_error)?;
        }
        let mut info = info.ok_or_else(|| Error::invalid("WAVE format is absent"))?;
        let (data_start, data_bytes) =
            data.ok_or_else(|| Error::invalid("WAVE sample data is absent"))?;
        let align = u64::from(info.channels) * u64::from(info.container_bits / 8);
        if data_bytes == 0 || data_bytes % align != 0 {
            return Err(Error::invalid("Incomplete or empty WAVE sample frame"));
        }
        info.frames = data_bytes / align;
        if info.frames > MAX_SAMPLE_FRAME {
            return Err(Error::limit("WAVE frame count exceeds domain budget"));
        }
        reader.seek(SeekFrom::Start(data_start)).map_err(io_error)?;
        Ok(Self {
            reader,
            info,
            data_start,
            data_bytes,
            frames_read: 0,
        })
    }
    pub fn info(&self) -> &WaveInfo {
        &self.info
    }
    pub fn seek_frame(&mut self, frame: u64) -> Result<()> {
        if frame > self.info.frames {
            return Err(Error::invalid("WAVE seek exceeds sample range"));
        }
        let offset =
            frame * u64::from(self.info.channels) * u64::from(self.info.container_bits / 8);
        if offset > self.data_bytes {
            return Err(Error::invalid("WAVE seek exceeds data chunk"));
        }
        self.reader
            .seek(SeekFrom::Start(self.data_start + offset))
            .map_err(io_error)?;
        self.frames_read = frame;
        Ok(())
    }
    pub fn read_frames(&mut self, maximum: usize) -> Result<Vec<f64>> {
        if maximum == 0 || maximum > 16384 {
            return Err(Error::limit("WAVE decoding block exceeds bounded buffer"));
        }
        let count = (self.info.frames - self.frames_read).min(maximum as u64) as usize;
        let width = usize::from(self.info.container_bits / 8);
        let mut raw = vec![0; count * usize::from(self.info.channels) * width];
        self.reader.read_exact(&mut raw).map_err(io_error)?;
        if !self.info.ieee_float && self.info.valid_bits < self.info.container_bits {
            let mask = (1_u64 << (self.info.container_bits - self.info.valid_bits)) - 1;
            for sample in raw.chunks_exact(width) {
                let mut value = 0_u64;
                for (index, byte) in sample.iter().enumerate() {
                    value |= u64::from(*byte) << (8 * index);
                }
                if value & mask != 0 {
                    return Err(Error::invalid(
                        "Nonzero padding bits in extensible PCM sample",
                    ));
                }
            }
        }
        let values = raw
            .chunks_exact(width)
            .map(|sample| {
                if self.info.ieee_float {
                    if width == 4 {
                        f64::from(f32::from_le_bytes(sample.try_into().expect("sample width")))
                    } else {
                        f64::from_le_bytes(sample.try_into().expect("sample width"))
                    }
                } else {
                    match width {
                        1 => (f64::from(sample[0]) - 128.0) / 128.0,
                        2 => {
                            f64::from(i16::from_le_bytes(sample.try_into().expect("sample width")))
                                / 32768.0
                        }
                        3 => {
                            f64::from(i32::from_le_bytes([0, sample[0], sample[1], sample[2]]) >> 8)
                                / 8388608.0
                        }
                        4 => {
                            f64::from(i32::from_le_bytes(sample.try_into().expect("sample width")))
                                / 2147483648.0
                        }
                        _ => unreachable!("validated PCM width"),
                    }
                }
            })
            .collect();
        self.frames_read += count as u64;
        Ok(values)
    }
    pub fn analyze(mut self, threshold_db: i32, minimum_silence: u64) -> Result<SignalStatistics> {
        self.seek_frame(0)?;
        let mut analyzer = PcmAnalyzer::new(
            self.info.sample_rate,
            self.info.channels,
            self.info.frames,
            threshold_db,
            minimum_silence,
        )?;
        loop {
            let block = self.read_frames(4096)?;
            if block.is_empty() {
                break;
            }
            analyzer.push_interleaved(&block)?;
        }
        analyzer.finish()
    }
}
