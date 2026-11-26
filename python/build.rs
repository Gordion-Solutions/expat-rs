//! On ELF platforms, bind the extension's calls to the `XML_*` functions it
//! exports to its own definitions. Without `-Bsymbolic` the dynamic linker
//! may resolve them to a libexpat already loaded into the process (a
//! library linked into the interpreter, or another extension's), and this
//! module would hand expat-rs parsers to libexpat's functions.
fn main() {
    let unix = std::env::var("CARGO_CFG_TARGET_FAMILY").is_ok_and(|f| f.split(',').any(|f| f == "unix"));
    let apple = std::env::var("CARGO_CFG_TARGET_VENDOR").is_ok_and(|v| v == "apple");
    if unix && !apple {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-Bsymbolic");
    }
}
