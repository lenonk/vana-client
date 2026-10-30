//! The Windows exe: its icon (assets/vana-client.rc) linked into the binaries, through rc.exe (MSVC) or windres (MinGW,
//! cross builds too; RC_<target> or RC names another compiler), and a 16 MB main-thread stack.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Windows gives the main thread 1 MB of stack (Linux 8): a debug build's frames overflow it at start.
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:16777216"),
        Ok("gnu") => println!("cargo:rustc-link-arg-bins=-Wl,--stack,16777216"),
        _ => {}
    }
    println!("cargo:rerun-if-changed=assets/vana-client.rc");
    println!("cargo:rerun-if-changed=assets/vana-client.ico");
    // rc.exe looks for the icon in its include dirs, not beside the .rc.
    let assets = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    if let Err(e) = embed_resource::compile("assets/vana-client.rc", embed_resource::ParamsIncludeDirs([assets])).manifest_optional() {
        println!("cargo:warning=no exe icon: {e}");
    }
}
