//! Compile-fail coverage for typed port connections.

#![allow(clippy::expect_used)]

use std::{fs, path::PathBuf, process::Command};

#[test]
fn incompatible_ports_do_not_compile() {
    let dependencies = std::env::current_exe()
        .expect("test executable path should be available")
        .parent()
        .expect("test executable should have a dependency directory")
        .to_path_buf();
    let sdk = newest_sdk_library(&dependencies);
    let output_directory =
        std::env::temp_dir().join(format!("logika-sdk-compile-fail-{}", std::process::id()));
    fs::create_dir_all(&output_directory).expect("compile-fail output directory should be created");

    let output = Command::new("rustc")
        .arg("--edition=2024")
        .arg("--crate-name=incompatible_ports")
        .arg("--emit=metadata")
        .arg("--out-dir")
        .arg(&output_directory)
        .arg("-L")
        .arg(format!("dependency={}", dependencies.display()))
        .arg("--extern")
        .arg(format!("logika_sdk={}", sdk.display()))
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/ui/incompatible_ports.rs"
        ))
        .output()
        .expect("rustc should run for the compile-fail fixture");

    assert!(!output.status.success(), "fixture unexpectedly compiled");
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("mismatched types"), "{diagnostic}");
    assert!(diagnostic.contains("Order"), "{diagnostic}");
    assert!(diagnostic.contains("Customer"), "{diagnostic}");
}

fn newest_sdk_library(dependencies: &std::path::Path) -> PathBuf {
    let mut candidates = fs::read_dir(dependencies)
        .expect("dependency directory should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("liblogika_sdk-") && name.ends_with(".rlib"))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    candidates
        .pop()
        .expect("compiled logika-sdk library should be available")
}
