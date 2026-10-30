//! Embed an rpath to ONNX Runtime (exported by aips-signal-sys via `links` metadata).
fn main() {
    if let Ok(dir) = std::env::var("DEP_AIPS_SIGNAL_ORT_LIB") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{dir}:$ORIGIN:/usr/local/lib");
    }
}
