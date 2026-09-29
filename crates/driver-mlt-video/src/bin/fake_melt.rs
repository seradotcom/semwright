//! Test-only ELF subprocess fixture. Never enabled by a production capability or runtime configuration.
use std::{
    io::{self, Write},
    process::{Command, Stdio},
    time::Duration,
};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "-version" {
        println!("melt 7.32.0 (Semwright fixture)");
        return;
    }
    if args.first().map(String::as_str) == Some("-query") {
        let values: &[&str] = match args.get(1).map(String::as_str) {
            Some("producers") => &["avformat", "color", "pixbuf"],
            Some("filters") => &["volume", "brightness"],
            Some("transitions") => &["mix", "luma"],
            Some("consumers") => &["avformat"],
            Some("video_codecs") => &["libx264", "ffv1"],
            Some("audio_codecs") => &["aac", "pcm_s16le"],
            _ => std::process::exit(2),
        };
        for value in values {
            println!("- {value}");
        }
        return;
    }
    let mode = args.first().map(String::as_str).unwrap_or("ok");
    match mode {
        "ok" => println!("completed"),
        "fail" => std::process::exit(7),
        "sleep" => std::thread::sleep(Duration::from_secs(60)),
        "flood" => {
            let block = [b'x'; 4096];
            loop {
                if io::stdout().write_all(&block).is_err() {
                    break;
                }
            }
        }
        "stderr-flood" => {
            let block = [b'x'; 4096];
            loop {
                if io::stderr().write_all(&block).is_err() {
                    break;
                }
            }
        }
        "echo" => {
            let json =
                semwright_mlt_video::json::array(args.iter().skip(1).cloned().map(Into::into));
            print!("{}", json.encode());
        }
        "env" => {
            print!(
                "{}",
                std::env::var("UNTRUSTED_TEST_SECRET").unwrap_or_else(|_| "absent".into())
            );
        }
        "cwd" => {
            print!("{}", std::env::current_dir().unwrap().display());
        }
        "partial" => {
            std::fs::write("partial.mkv", b"NOT A MEDIA FILE").unwrap();
            std::process::exit(9);
        }
        "descendant" => {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .arg("sleep")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            std::fs::write("descendant.pid", child.id().to_string()).unwrap();
            let _ = child.wait();
            // Keep the fixture alive if an emulator or constrained CI runner reaps the
            // descendant early. The runtime must still take its timeout path and clean
            // the complete process group.
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        "query" => {
            println!("---\nfilters:\n  - volume\n  - brightness\n  - bad service\n...");
        }
        _ => std::process::exit(2),
    }
}
