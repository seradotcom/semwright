use std::{env, path::PathBuf, process::Command};
fn main() {
    for f in [
        "Model.swift",
        "Native.swift",
        "Accessibility.swift",
        "Input.swift",
        "Capture.swift",
        "Platform.swift",
    ] {
        println!("cargo:rerun-if-changed=native/{f}");
    }
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "macos"
        || env::var_os("CARGO_FEATURE_NATIVE").is_none()
    {
        return;
    }
    if env::var("HOST").unwrap_or_default() != env::var("TARGET").unwrap_or_default() {
        panic!(
            "Native Swift framework build requires a matching Apple toolchain; use --no-default-features for Rust-only metadata checks, not native verification"
        );
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => panic!("Unsupported Mac architecture"),
    };

    println!("cargo:rerun-if-changed=native/SecureInput.c");
    assert!(
        Command::new("xcrun")
            .args([
                "clang",
                "-arch",
                arch,
                "-mmacosx-version-min=14.0",
                "-c",
                "native/SecureInput.c",
                "-o"
            ])
            .arg(out.join("SecureInput.o"))
            .status()
            .expect("Apple C compiler")
            .success(),
        "Secure input wrapper failed"
    );
    let mut c = Command::new("xcrun");
    c.arg("swiftc");
    c.args(["-I", "native/include"]);
    println!("cargo:rerun-if-changed=native/include");
    c.arg(out.join("SecureInput.o"));

    c.args([
        "-swift-version",
        "5",
        "-parse-as-library",
        "-emit-library",
        "-module-name",
        "SemwrightNative",
        "-target",
        &format!("{arch}-apple-macosx14.0"),
    ]);
    for f in [
        "Model.swift",
        "Native.swift",
        "Accessibility.swift",
        "Input.swift",
        "Capture.swift",
        "Platform.swift",
    ] {
        c.arg(format!("native/{f}"));
    }
    for framework in [
        "AppKit",
        "ApplicationServices",
        "CoreGraphics",
        "ScreenCaptureKit",
        "ImageIO",
        "UniformTypeIdentifiers",
        "Security",
        "ServiceManagement",
        "Carbon",
    ] {
        c.args(["-framework", framework]);
    }
    c.args([
        "-Xlinker",
        "-install_name",
        "-Xlinker",
        "@rpath/libSemwrightNative.dylib",
        "-o",
    ])
    .arg(out.join("libSemwrightNative.dylib"));
    assert!(
        c.status()
            .expect("Xcode Swift toolchain required")
            .success(),
        "Swift native host build failed"
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib=SemwrightNative");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    if env::var("PROFILE").as_deref() != Ok("release") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", out.display());
    }
}
