fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // This belongs to the final executable, not a library build script:
        // dependency rustc-link-arg metadata is not a substitute for a binary rpath.
        println!("cargo:rustc-link-arg-bin=semwrightd=-Wl,-rpath,@executable_path/../Frameworks");
    }
}
