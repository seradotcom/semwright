use std::io::{Read, Write};

fn main() {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("fixture stdin");
    let tag = std::env::var("SEMWRIGHT_FIXTURE").unwrap_or_default();
    let cpu_ms = std::env::var("SEMWRIGHT_CPU_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
        .min(5_000);
    if cpu_ms != 0 {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(cpu_ms);
        let mut state = 0x517c_c1b7_u64;
        while std::time::Instant::now() < deadline {
            for _ in 0..16_384 {
                state = state
                    .wrapping_mul(2_862_933_555_777_941_757)
                    .wrapping_add(3_037_000_493);
            }
            std::hint::black_box(state);
        }
    }
    let path_visible = std::env::var_os("PATH").is_some();
    let mut output = std::io::stdout().lock();
    write!(output, "{tag}|path={path_visible}|").expect("fixture prefix");
    output.write_all(&input).expect("fixture echo");
    output.flush().expect("fixture flush");
}
