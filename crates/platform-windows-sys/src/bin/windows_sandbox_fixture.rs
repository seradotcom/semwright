use std::io::{Read, Write};

fn main() {
    let mut input = Vec::new();
    std::io::stdin()
        .read_to_end(&mut input)
        .expect("fixture stdin");
    let tag = std::env::var("SEMWRIGHT_FIXTURE").unwrap_or_default();
    let path_visible = std::env::var_os("PATH").is_some();
    let mut output = std::io::stdout().lock();
    write!(output, "{tag}|path={path_visible}|").expect("fixture prefix");
    output.write_all(&input).expect("fixture echo");
    output.flush().expect("fixture flush");
}
