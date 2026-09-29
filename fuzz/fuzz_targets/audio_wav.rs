#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_audio_domain::wav::WaveReader;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1_048_576 {
        return;
    }
    if let Ok(mut reader) = WaveReader::open(Cursor::new(data), 1_048_576) {
        let info = reader.info().clone();
        assert!(info.frames > 0);
        let _ = reader.read_frames(1);
        let _ = reader.seek_frame(info.frames / 2);
        let _ = reader.read_frames(257);
        let _ = reader.analyze(-90_000, 1);
    }
});
