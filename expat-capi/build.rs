//! Link settings for the shared library: give it libexpat's names so
//! programs built against the real libexpat load it unchanged, and on macOS
//! export only the XML_* symbols (Rust runtime symbols stay hidden, so two
//! Rust libraries in one process don't collide). On Linux and Android rustc
//! already exports only the XML_* functions, unversioned like libexpat's; a
//! version script of our own would clash with rustc's under GNU ld.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libexpat.1.dylib");
            println!("cargo:rustc-cdylib-link-arg=-Wl,-exported_symbols_list,{dir}/exported_symbols.txt");
        }
        Ok("linux") => {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libexpat.so.1");
        }
        // AOSP's libexpat is plain libexpat.so.
        Ok("android") => println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libexpat.so"),
        _ => {}
    }
    println!("cargo:rerun-if-changed=exported_symbols.txt");
}
