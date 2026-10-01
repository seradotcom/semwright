use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

fn main() {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let ro = args.next().expect("ro root");
    let rw = args.next().expect("rw root");
    let denied = args.next().expect("denied root");

    let read_ok = fs::read_to_string(ro.join("input.txt")).is_ok_and(|value| value == "allowed-ro");
    let ro_write_denied = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(ro.join("blocked.txt"))
        .is_err();
    let write_ok = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(rw.join("output.txt"))
        .and_then(|mut file| file.write_all(b"written"))
        .is_ok();
    let denied_ok = fs::read_to_string(denied.join("secret.txt")).is_err();
    let sandbox = std::env::var("SEMWRIGHT_DRIVER_SANDBOX").unwrap_or_default();

    println!(
        "read_ok={read_ok}|ro_write_denied={ro_write_denied}|write_ok={write_ok}|denied_ok={denied_ok}|sandbox={sandbox}"
    );
    if !(read_ok && ro_write_denied && write_ok && denied_ok && sandbox == "macos-app-sandbox-v1") {
        std::process::exit(10);
    }
}
