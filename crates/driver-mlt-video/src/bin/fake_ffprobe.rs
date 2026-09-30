//! Narrow ffprobe fixture used only by runtime-runner contract tests.

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 7
        || args[0] != "-v"
        || args[1] != "error"
        || args[2] != "-show_streams"
        || args[3] != "-show_format"
        || args[4] != "-of"
        || args[5] != "json"
    {
        eprintln!("unexpected fake ffprobe arguments");
        std::process::exit(2);
    }
    let input = std::path::Path::new(&args[6]);
    if !input.is_absolute() || !input.is_file() {
        eprintln!("fake ffprobe input is unavailable");
        std::process::exit(3);
    }
    print!(
        r#"{{"streams":[{{"codec_type":"video","codec_name":"ffv1","width":160,"height":90,"duration_ts":50,"time_base":"1/25","nb_frames":"50"}},{{"codec_type":"audio","codec_name":"pcm_s16le","duration_ts":96000,"time_base":"1/48000"}}],"format":{{"duration":"2.000000000"}}}}"#
    );
}
