fn main() {
    if semwright_faust_audio::meter_snapshot::run().is_err() {
        eprintln!("Pinned audio meter snapshot failed");
        std::process::exit(1);
    }
}
