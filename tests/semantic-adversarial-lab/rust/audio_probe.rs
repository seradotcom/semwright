//! Lab-owned synthetic PCM/WAVE adversarial inputs; no audio device or playback.
use semwright_audio_domain::{Result, signal_analysis::PcmAnalyzer, time::SampleRate, wav::WaveReader};
use serde_json::{Value,json};
use std::io::Cursor;
const SOURCE:&str=env!("G_LAB_COMPILED_SOURCE_SHA");
fn analyzer(channels:u16,frames:u64)->Result<PcmAnalyzer>{PcmAnalyzer::new(SampleRate::new(48000)?,channels,frames,-60000,1)}
fn chunk(id:&[u8;4], data:&[u8])->Vec<u8>{let mut b=id.to_vec();b.extend_from_slice(&(data.len() as u32).to_le_bytes());b.extend_from_slice(data);if data.len()%2==1{b.push(0);}b}
fn fmt(tag:u16,channels:u16,bits:u16)->Vec<u8>{
    let mut b=vec![];b.extend_from_slice(&tag.to_le_bytes());b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&48000_u32.to_le_bytes());let align=channels*(bits/8);
    b.extend_from_slice(&(48000*u32::from(align)).to_le_bytes());b.extend_from_slice(&align.to_le_bytes());b.extend_from_slice(&bits.to_le_bytes());b
}
fn riff(chunks:Vec<Vec<u8>>)->Vec<u8>{let mut body=b"WAVE".to_vec();for c in chunks{body.extend(c);}let mut b=b"RIFF".to_vec();b.extend_from_slice(&(body.len() as u32).to_le_bytes());b.extend(body);b}
fn pcm()->Vec<u8>{[-32768_i16,0,16384,32767].iter().flat_map(|s|s.to_le_bytes()).collect()}
fn wave()->Vec<u8>{riff(vec![chunk(b"fmt ",&fmt(1,1,16)),chunk(b"data",&pcm())])}
fn opened(b:Vec<u8>)->bool{WaveReader::open(Cursor::new(b),65536).is_ok()}
fn probe(id:&str)->Result<Value>{Ok(match id{
    "G-AUDIO-001"=>{let mut a=analyzer(1,4)?;a.push_interleaved(&[0.0,0.5,-0.5,0.0])?;let r=a.finish()?;json!({"frames":r.frames,"nonfinite":r.nonfinite_samples,"peak_millidbfs":r.peak_millidbfs,"rms_millidbfs":r.rms_millidbfs,"entirely_silent":r.entirely_silent})},
    "G-AUDIO-002"=>{let mut a=analyzer(2,8)?;a.push_interleaved(&[0.0;16])?;let r=a.finish()?;json!({"frames":r.frames,"peak":r.peak_millidbfs,"rms":r.rms_millidbfs,"silent":r.entirely_silent,"ranges":r.silence_ranges.len(),"exhaustive":r.silence_ranges_exhaustive})},
    "G-AUDIO-003"=>{let mut a=analyzer(1,3)?;a.push_interleaved(&[f64::NAN,f64::INFINITY,f64::NEG_INFINITY])?;let r=a.finish()?;json!({"nonfinite":r.nonfinite_samples,"finite":r.channels[0].finite_samples,"silent":r.entirely_silent,"peak":r.peak_millidbfs,"rms":r.rms_millidbfs,"dc":r.channels[0].dc_offset})},
    "G-AUDIO-004"=>{let mut a=analyzer(1,3)?;a.push_interleaved(&[0.5,f64::NAN,-0.5])?;let r=a.finish()?;json!({"nonfinite":r.nonfinite_samples,"finite":r.channels[0].finite_samples,"peak":r.peak_millidbfs,"rms":r.rms_millidbfs,"dc":r.channels[0].dc_offset})},
    "G-AUDIO-005"=>{let mut a=analyzer(2,2)?;let denied=a.push_interleaved(&[0.1,0.2,0.3]).is_err();a.push_interleaved(&[0.0,0.0])?;json!({"partial_frame_denied":denied,"frames_after_valid":a.finish()?.frames})},
    "G-AUDIO-006"=>{let mut a=analyzer(1,2)?;a.push_interleaved(&[0.0])?;let denied=a.push_interleaved(&[0.0,0.0]).is_err();a.push_interleaved(&[0.0])?;json!({"over_budget_denied":denied,"frames":a.finish()?.frames})},
    "G-AUDIO-007"=>{let mut a=analyzer(1,2)?;let denied=a.push_interleaved(&[0.25,f64::MAX]).is_err();a.push_interleaved(&[0.0])?;let r=a.finish()?;json!({"huge_finite_denied":denied,"frames":r.frames,"silent":r.entirely_silent})},
    "G-AUDIO-008"=>{let mut a=analyzer(1,4)?;a.push_interleaved(&[-1.01,-1.0,1.0,1.01])?;let r=a.finish()?;json!({"out_of_range":r.out_of_range_samples,"nonfinite":r.nonfinite_samples})},
    "G-AUDIO-009"=>{let values=[0.0,0.0,0.25,0.0,0.0,0.0,-0.25,0.0];let mut a=analyzer(1,8)?;a.push_interleaved(&values)?;let mut b=analyzer(1,8)?;for part in values.chunks(3){b.push_interleaved(part)?;}json!({"streaming_partition_invariant":a.finish()?==b.finish()?})},
    "G-AUDIO-010"=>{let mut a=analyzer(1,8200)?;let v:Vec<f64>=(0..8200).map(|n|if n%2==0{0.0}else{0.5}).collect();a.push_interleaved(&v)?;let r=a.finish()?;json!({"stored_ranges":r.silence_ranges.len(),"exhaustive":r.silence_ranges_exhaustive})},
    "G-AUDIO-011"=>json!({"empty_analysis_denied":analyzer(1,2)?.finish().is_err()}),
    "G-AUDIO-012"=>{let rate=SampleRate::new(48000)?;let results=[PcmAnalyzer::new(rate,0,1,-60000,1).is_err(),PcmAnalyzer::new(rate,65,1,-60000,1).is_err(),PcmAnalyzer::new(rate,1,0,-60000,1).is_err(),PcmAnalyzer::new(rate,1,1,-180001,1).is_err(),PcmAnalyzer::new(rate,1,1,-60000,0).is_err()];json!({"invalid_configurations_denied":results.into_iter().filter(|v|*v).count()})},
    "G-AUDIO-013"=>{let mut a=analyzer(2,2)?;a.push_interleaved(&[0.0,1.0,0.0,-1.0])?;let r=a.finish()?;json!({"left_peak":r.channels[0].peak_millidbfs,"right_peak":r.channels[1].peak_millidbfs,"silent":r.entirely_silent,"silence_ranges":r.silence_ranges.len()})},
    "G-AUDIO-014"=>{let mut a=analyzer(1,1)?;a.push_interleaved(&[0.0])?;let r=serde_json::to_value(a.finish()?).expect("finite statistics");json!({"claims_lufs":r.get("lufs").is_some(),"claims_true_peak":r.get("true_peak").is_some(),"claims_intelligibility":r.get("intelligibility").is_some()})},
    "G-AUDIO-015"=>{let mut a=analyzer(1,2)?;a.push_interleaved(&[-0.0,0.0])?;let r=a.finish()?;json!({"signed_zero_silent":r.entirely_silent,"peak":r.peak_millidbfs})},
    "G-WAVE-001"=>{let mut w=WaveReader::open(Cursor::new(wave()),65536)?;let values=w.read_frames(4)?;json!({"frames":w.info().frames,"decoded":values,"eof":w.read_frames(1)?.is_empty()})},
    "G-WAVE-002"=>{let good=wave();let mut denied=0;for n in 0..good.len(){denied+=usize::from(!opened(good[..n].to_vec()));}json!({"truncations_rejected":denied,"bytes":good.len()})},
    "G-WAVE-003"=>{let mut b=wave();b[4..8].copy_from_slice(&0_u32.to_le_bytes());json!({"size_mismatch_rejected":!opened(b)})},
    "G-WAVE-004"=>json!({"duplicate_data_rejected":!opened(riff(vec![chunk(b"fmt ",&fmt(1,1,16)),chunk(b"data",&pcm()),chunk(b"data",&pcm())]))}),
    "G-WAVE-005"=>json!({"duplicate_format_rejected":!opened(riff(vec![chunk(b"fmt ",&fmt(1,1,16)),chunk(b"fmt ",&fmt(1,1,16)),chunk(b"data",&pcm())]))}),
    "G-WAVE-006"=>{let mut f=fmt(1,1,16);f[12..14].copy_from_slice(&1_u16.to_le_bytes());json!({"alignment_rejected":!opened(riff(vec![chunk(b"fmt ",&f),chunk(b"data",&pcm())]))})},
    "G-WAVE-007"=>{let mut f=fmt(1,1,16);f[8..12].copy_from_slice(&0_u32.to_le_bytes());json!({"byte_rate_rejected":!opened(riff(vec![chunk(b"fmt ",&f),chunk(b"data",&pcm())]))})},
    "G-WAVE-008"=>json!({"partial_sample_rejected":!opened(riff(vec![chunk(b"fmt ",&fmt(1,1,16)),chunk(b"data",&[0])]))}),
    "G-WAVE-009"=>json!({"missing_format_rejected":!opened(riff(vec![chunk(b"data",&pcm()),chunk(b"JUNK",&[0;32])]))}),
    "G-WAVE-010"=>{let data=wave();let length=data.len() as u64;json!({"input_budget_rejected":WaveReader::open(Cursor::new(data),length-1).is_err()})},
    "G-WAVE-011"=>{let b=riff(vec![chunk(b"JUNK",&[1,2,3]),chunk(b"fmt ",&fmt(1,1,16)),chunk(b"data",&pcm())]);let mut w=WaveReader::open(Cursor::new(b),65536)?;json!({"odd_unknown_chunk_preserved_samples":w.read_frames(4)?})},
    "G-WAVE-012"=>{let mut w=WaveReader::open(Cursor::new(wave()),65536)?;w.seek_frame(2)?;let frame=w.read_frames(1)?;let beyond=w.seek_frame(5).is_err();w.seek_frame(4)?;json!({"seek_frame":frame,"beyond_end_denied":beyond,"seek_end_is_eof":w.read_frames(1)?.is_empty()})},
    "G-WAVE-013"=>{let mut w=WaveReader::open(Cursor::new(wave()),65536)?;json!({"zero_buffer_denied":w.read_frames(0).is_err(),"oversize_buffer_denied":w.read_frames(16385).is_err(),"no_consumption":w.read_frames(1)?})},
    "G-WAVE-014"=>{let bytes:Vec<u8>=[f64::NAN,f64::INFINITY,0.0].into_iter().flat_map(f64::to_le_bytes).collect();let b=riff(vec![chunk(b"fmt ",&fmt(3,1,64)),chunk(b"data",&bytes)]);let r=WaveReader::open(Cursor::new(b),65536)?.analyze(-60000,1)?;json!({"decoded_nonfinite":r.nonfinite_samples,"finite":r.channels[0].finite_samples,"silent":r.entirely_silent})},
    "G-WAVE-015"=>{let mut seed=739_u64;let mut attempts=0;for _ in 0..128{let mut b=wave();seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);let index=(seed as usize)%b.len();b[index]^=((seed>>32) as u8)|1;if let Ok(mut w)=WaveReader::open(Cursor::new(b),65536){let _=w.read_frames(16);}attempts+=1;}json!({"bounded_parser_mutations_completed":attempts})},
    _=>{eprintln!("unregistered audio selector");std::process::exit(2)}
})}
fn cases()->Vec<String>{[("AUDIO",15),("WAVE",15)].into_iter().flat_map(|(f,n)|(1..=n).map(move|i|format!("G-{f}-{i:03}"))).collect()}
fn main(){let args:Vec<_>=std::env::args().skip(1).collect();if args.len()!=1||std::env::var("G_LAB_TARGET_SHA").as_deref()!=Ok(SOURCE){std::process::exit(2)}
if args[0]=="--list"{println!("{}",json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()}));return;}
if !cases().contains(&args[0]){std::process::exit(2)}
match probe(&args[0]){Ok(observed)=>println!("{}",json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})),Err(e)=>{eprintln!("audio probe contract/setup error: {e:?}");std::process::exit(1)}}}
