//! Link settings for the shared library: export only the XML_* symbols
//! (Rust runtime symbols stay hidden, so two Rust libraries in one process
//! don't collide), and give it libexpat's names so programs built against
//! the real libexpat load it unchanged.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libexpat.1.dylib");
            println!("cargo:rustc-cdylib-link-arg=-Wl,-exported_symbols_list,{dir}/exported_symbols.txt");
        }
        Ok("linux") => {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libexpat.so.1");
            println!("cargo:rustc-cdylib-link-arg=-Wl,--version-script={dir}/expat.map");
        }
        _ => {}
    }
    println!("cargo:rerun-if-changed=exported_symbols.txt");
    println!("cargo:rerun-if-changed=expat.map");
}
