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

/// The directory holding the libexpat.a built for this test run: the test
/// executable's own (`target/<profile>/deps/`). Not `target/<profile>/`,
/// which only `cargo build` updates, so it may hold a stale library.
fn lib_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    exe.parent().expect("target/<profile>/deps").to_path_buf()
}

fn check_probe(name: &str) {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = here.join("tests/c").join(format!("{name}.c"));
    let expected = std::fs::read_to_string(here.join("tests/c").join(format!("{name}.expected"))).expect("expected output");
    let lib = lib_dir().join("libexpat.a");
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
