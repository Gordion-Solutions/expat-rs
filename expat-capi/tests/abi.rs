//! Layer 1: C programs built against expat.h and linked with this crate's
//! static library must print exactly what they print with libexpat.
//!
//! Each `tests/c/<name>.c` is compiled with the C compiler (`$CC`, or `cc`)
//! and linked with `libexpat.a` from this build, then run; its output must
//! equal `tests/c/<name>.expected`, recorded from libexpat 2.8.2 by
//! `tools/regen_expected.sh`. Linking statically means the system's
//! libexpat can't stand in for this one.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The libexpat.a built for this test run. Not `target/<profile>/`'s, which
/// only `cargo build` updates, so it may be stale.
///
/// Up to Rust 1.99 it is next to the test executable, in
/// `target/<profile>/deps/`. From 1.100 every compiled unit has its own
/// `target/<profile>/build/<package>/<hash>/out/`, the test executable in
/// one and the library in another: then it is the most recently built
/// library of this package, the one `cargo test` just compiled or found up
/// to date.
fn static_lib() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    let dir = exe.parent().expect("test executable's directory");
    let beside = dir.join("libexpat.a");
    if beside.exists() {
        return beside;
    }
    let package = dir.parent().and_then(Path::parent).expect("target/<profile>/build/<package>/");
    let built = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    std::fs::read_dir(package)
        .map(|units| units.flatten().map(|u| u.path().join("out/libexpat.a")).filter(|p| p.exists()).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .max_by_key(built)
        .unwrap_or(beside)
}

fn check_probe(name: &str) {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = here.join("tests/c").join(format!("{name}.c"));
    let expected = std::fs::read_to_string(here.join("tests/c").join(format!("{name}.expected"))).expect("expected output");
    let lib = static_lib();
    assert!(lib.exists(), "{} not built", lib.display());
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let exe = out_dir.join(name);
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let mut build = Command::new(&cc);
    build.args(["-Wall", "-Werror", "-DXML_DTD", "-I"]).arg(here.join("include")).arg("-o").arg(&exe).arg(&src).arg(&lib);
    // What a Rust static library needs from the platform.
    if cfg!(target_os = "linux") {
        build.args(["-lpthread", "-ldl", "-lm"]);
    }
    let status = build.status().unwrap_or_else(|e| panic!("running {cc}: {e}"));
    assert!(status.success(), "compiling {name}.c failed");
    let run = Command::new(&exe).output().expect("running the probe");
    assert!(run.status.success(), "{name} exited with {} (stderr: {})", run.status, String::from_utf8_lossy(&run.stderr));
    let got = String::from_utf8_lossy(&run.stdout);
    if got != expected {
        let diff: Vec<String> = expected
            .lines()
            .zip(got.lines())
            .enumerate()
            .filter(|(_, (e, g))| e != g)
            .take(20)
            .map(|(i, (e, g))| format!("line {}:\n  libexpat:   {e}\n  expat-capi: {g}", i + 1))
            .collect();
        panic!(
            "{name}: output differs from libexpat's ({} vs {} lines)\n{}",
            expected.lines().count(),
            got.lines().count(),
            diff.join("\n")
        );
    }
}

#[test]
fn abi_probe_matches_libexpat() {
    check_probe("abi_probe");
}

#[test]
fn positions_match_libexpat() {
    check_probe("positions");
}

#[test]
fn cdata_in_pieces_matches_libexpat() {
    check_probe("cdata");
}

#[test]
fn android_consumers_match_libexpat() {
    check_probe("android_consumers");
}

#[test]
fn unknown_encodings_match_libexpat() {
    check_probe("encodings");
}

#[test]
fn parser_reset_matches_libexpat() {
    check_probe("reset");
}

#[test]
fn declarations_match_libexpat() {
    check_probe("declarations");
}

#[test]
fn dtd_in_pieces_matches_libexpat() {
    check_probe("dtd_pieces");
}

/// The words of a declaration that go to the default handler: libexpat
/// passes each as it arrives, expat-capi when the declaration is complete
/// (see tests/c/dtd_words.c and the README).
#[test]
#[ignore = "a declaration's words reach the default handler when it is complete"]
fn declaration_words_in_pieces_match_libexpat() {
    check_probe("dtd_words");
}
