use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=native/confined.c");
    println!("cargo:rerun-if-changed=native/confined.h");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "macos" && os != "linux" {
        return;
    }
    let host = env::var("HOST").expect("Cargo HOST");
    let target = env::var("TARGET").expect("Cargo TARGET");
    if os == "macos" && !host.contains("apple-darwin") {
        println!(
            "cargo:warning=Skipping Darwin C object during non-Apple cross-check; native linking remains a macOS-only verification gate"
        );
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let mut cc = if os == "macos" {
        let mut c = Command::new("xcrun");
        c.args([
            "--sdk",
            "macosx",
            "clang",
            "-arch",
            if target.starts_with("aarch64") {
                "arm64"
            } else {
                "x86_64"
            },
            "-mmacosx-version-min=14.0",
        ]);
        c
    } else {
        Command::new(env::var("CC").unwrap_or_else(|_| "cc".into()))
    };
    let status = cc
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-fPIC",
            "-c",
            "native/confined.c",
            "-o",
        ])
        .arg(out.join("confined.o"))
        .status()
        .expect("C compiler required");
    assert!(
        status.success(),
        "Descriptor confinement C compilation failed"
    );
    let status = Command::new(env::var("AR").unwrap_or_else(|_| "ar".into()))
        .arg("rcs")
        .arg(out.join("libsemwright_confined.a"))
        .arg(out.join("confined.o"))
        .status()
        .expect("Archiver required");
    assert!(status.success(), "Confinement archive failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=semwright_confined");
}
