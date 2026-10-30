//! Builds libs/aips-signal (C++17) with CMake and links ONNX Runtime.
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..").canonicalize().unwrap();
    let src = root.join("libs/aips-signal");
    let ort = std::env::var("AIPS_ONNXRUNTIME_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("third_party/onnxruntime-linux-x64-1.20.1"));

    let mut cfg = cmake::Config::new(&src);
    cfg.define("AIPS_BUILD_TESTS", "OFF").profile("Release");
    if ort.join("include/onnxruntime_cxx_api.h").exists() {
        cfg.define("ONNXRUNTIME_ROOT", &ort);
        println!(
            "cargo:rustc-link-search=native={}",
            ort.join("lib").display()
        );
        println!("cargo:rustc-link-lib=dylib=onnxruntime");
        println!(
            "cargo:rustc-link-arg=-Wl,-rpath,{}",
            ort.join("lib").display()
        );
        println!("cargo:ort_lib={}", ort.join("lib").display());
    } else {
        println!(
            "cargo:warning=ONNX Runtime not found at {}; RUL inference disabled",
            ort.display()
        );
    }
    if std::env::var("AIPS_ENABLE_CUDA")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        cfg.define("AIPS_ENABLE_CUDA", "ON");
        println!("cargo:rustc-link-lib=dylib=cudart");
    }
    let dst = cfg.build();
    println!("cargo:rustc-link-search=native={}/lib", dst.display());
    println!("cargo:rustc-link-lib=static=aips_signal");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-env-changed=AIPS_ONNXRUNTIME_ROOT");
    println!("cargo:rerun-if-env-changed=AIPS_ENABLE_CUDA");
}
