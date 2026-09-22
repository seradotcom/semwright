// SPDX-License-Identifier: GPL-3.0-or-later
//! Build-time Go compilation only: the driver never launches a helper at runtime.
use std::{env, path::PathBuf, process::Command};

fn main() {
    let target = env::var("TARGET").expect("Cargo TARGET");
    let host = env::var("HOST").expect("Cargo HOST");
    assert!(
        target.contains("linux") && target == host,
        "Native Linux builds only; cross-compilation is not certified"
    );
    let source =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest path")).join("../native");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let go_cache = output.join("go-cache");
    let go_tmp = output.join("go-tmp");
    std::fs::create_dir_all(&go_cache).expect("create isolated Go build cache");
    std::fs::create_dir_all(&go_tmp).expect("create isolated Go temporary directory");
    let status = Command::new("go")
        .current_dir(&source)
        .env("GOTOOLCHAIN", "local")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOCACHE", &go_cache)
        .env("GOTMPDIR", &go_tmp)
        .env("CGO_ENABLED", "1")
        .args(["build", "-trimpath", "-buildmode=c-archive", "-o"])
        .arg(output.join("libkicadcore.a"))
        .arg("./cmd/kicad-core")
        .status()
        .expect("Go 1.23+ and a C compiler are required to build the linked native IPC core");
    assert!(status.success(), "Native IPC core archive build failed");
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=kicadcore");
    for library in ["pthread", "dl", "m", "resolv"] {
        println!("cargo:rustc-link-lib={library}");
    }
    println!("cargo:rerun-if-changed=../native");
    println!("cargo:rerun-if-changed=build.rs");
}
