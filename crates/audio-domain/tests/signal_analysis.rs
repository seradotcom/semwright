use proptest::prelude::*;
use semwright_audio_domain::{signal_analysis::PcmAnalyzer, time::SampleRate, wav::WaveReader};
use std::io::Cursor;
fn wave(tag: u16, bits: u16, channels: u16, data: &[u8]) -> Vec<u8> {
    let rate = 48000_u32;
    let align = channels * (bits / 8);
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36_u32 + data.len() as u32 + data.len() as u32 % 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(tag.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(align)).to_le_bytes());
    bytes.extend(align.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((data.len() as u32).to_le_bytes());
    bytes.extend(data);
    if data.len() % 2 != 0 {
        bytes.push(0);
    }
    bytes
}
#[test]
fn silence_has_no_fabricated_finite_level() {
    let mut a = PcmAnalyzer::new(SampleRate(48000), 2, 10, -90000, 1).unwrap();
    a.push_interleaved(&[0.0; 20]).unwrap();
    let r = a.finish().unwrap();
    assert!(r.entirely_silent);
    assert_eq!(r.peak_millidbfs, None);
    assert_eq!(r.rms_millidbfs, None);
    assert_eq!(r.silence_ranges[0].end_frame, 10);
    assert_eq!(r.channels[0].dc_offset, Some(0.0));
    assert!(serde_json::to_string(&r).unwrap().contains("null"));
}
#[test]
fn corrupt_float_samples_are_counted_not_serialized_as_nan() {
    let mut a = PcmAnalyzer::new(SampleRate(48000), 1, 4, -90000, 1).unwrap();
    a.push_interleaved(&[f64::NAN, f64::INFINITY, -2.0, 0.5])
        .unwrap();
    let r = a.finish().unwrap();
    assert_eq!(r.nonfinite_samples, 2);
    assert_eq!(r.out_of_range_samples, 1);
    assert!(!r.entirely_silent);
    assert_eq!(r.peak_millidbfs, Some(6021));
    assert!(!serde_json::to_string(&r).unwrap().contains("NaN"));
}
#[test]
fn bounded_silence_inventory_does_not_claim_exhaustive() {
    let mut a = PcmAnalyzer::new(SampleRate(48000), 1, 10000, -90000, 1).unwrap();
    for _ in 0..5000 {
        a.push_interleaved(&[0.0, 0.5]).unwrap();
    }
    let r = a.finish().unwrap();
    assert_eq!(r.silence_ranges.len(), 4096);
    assert!(!r.silence_ranges_exhaustive);
}
#[test]
fn invalid_block_does_not_consume_budget() {
    let mut a = PcmAnalyzer::new(SampleRate(48000), 2, 2, -90000, 1).unwrap();
    assert!(a.push_interleaved(&[1.0]).is_err());
    assert!(a.push_interleaved(&[0.0; 6]).is_err());
    a.push_interleaved(&[0.0; 4]).unwrap();
    assert_eq!(a.finish().unwrap().frames, 2);
}
#[test]
fn wave_decodes_pcm24_and_float_without_clamping() {
    let mut r = WaveReader::open(
        Cursor::new(wave(1, 24, 1, &[0, 0, 0x80, 0xff, 0xff, 0x7f])),
        1024,
    )
    .unwrap();
    let samples = r.read_frames(2).unwrap();
    assert_eq!(samples[0], -1.0);
    assert!(samples[1] > 0.9999);
    let mut r =
        WaveReader::open(Cursor::new(wave(3, 32, 1, &2.0_f32.to_le_bytes())), 1024).unwrap();
    assert_eq!(r.read_frames(1).unwrap(), vec![2.0]);
}
#[test]
fn wave_truncation_alignment_and_budget_fail_closed() {
    let bytes = wave(1, 16, 1, &[0; 8]);
    assert!(WaveReader::open(Cursor::new(&bytes), 20).is_err());
    assert!(WaveReader::open(Cursor::new(&bytes[..bytes.len() - 1]), 1024).is_err());
    let mut bad = bytes.clone();
    bad[32] = 3;
    assert!(WaveReader::open(Cursor::new(bad), 1024).is_err());
    assert!(WaveReader::open(Cursor::new(wave(1, 16, 2, &[0; 6])), 1024).is_err());
}
#[test]
fn wave_analysis_rewinds_and_rejects_out_of_range_seek() {
    let bytes = wave(1, 16, 1, &[0; 20]);
    let mut r = WaveReader::open(Cursor::new(bytes), 1024).unwrap();
    assert!(r.seek_frame(11).is_err());
    r.seek_frame(5).unwrap();
    assert_eq!(r.analyze(-90000, 1).unwrap().frames, 10);
}
proptest! {
    #[test]
    fn pcm16_roundtrip_streaming(samples in prop::collection::vec(any::<i16>(), 1..1024)) {
        let raw: Vec<_> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut reader = WaveReader::open(Cursor::new(wave(1,16,1,&raw)), 10000).unwrap();
        let mut decoded = Vec::new();
        loop { let block = reader.read_frames(7).unwrap(); if block.is_empty() { break; } decoded.extend(block); }
        prop_assert_eq!(decoded.len(), samples.len());
        for (actual, expected) in decoded.iter().zip(samples.iter()) { prop_assert_eq!(*actual, f64::from(*expected)/32768.0); }
    }
    #[test]
    fn chunking_preserves_statistics(samples in prop::collection::vec(-32768_i16..32767, 1..1000)) {
        let values: Vec<_> = samples.iter().map(|v| f64::from(*v)/32768.0).collect();
        let mut all = PcmAnalyzer::new(SampleRate(48000),1,1000,-90000,1).unwrap();
        let mut chunks = PcmAnalyzer::new(SampleRate(48000),1,1000,-90000,1).unwrap();
        all.push_interleaved(&values).unwrap(); for chunk in values.chunks(11) { chunks.push_interleaved(chunk).unwrap(); }
        prop_assert_eq!(all.finish().unwrap(), chunks.finish().unwrap());
    }
}
