//! Command-line acceptance scenarios for Orbita 0.1.

#![allow(clippy::expect_used)]

use std::{path::PathBuf, process::Command};

use serde_json::Value;

#[test]
fn validates_a_local_workflow_with_human_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_orbita"))
        .arg("validate")
        .arg(fixture("empty-workflow.yaml"))
        .output()
        .expect("orbita should start");

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("is valid"));
    assert!(output.stderr.is_empty());
}

#[test]
fn emits_machine_readable_diagnostics_and_release_version() {
    let invalid = Command::new(env!("CARGO_BIN_EXE_orbita"))
        .arg("validate")
        .arg(fixture("unresolved-workflow.json"))
        .arg("--json")
        .output()
        .expect("orbita should start");
    let version = Command::new(env!("CARGO_BIN_EXE_orbita"))
        .arg("--version")
        .output()
        .expect("orbita should report its version");

    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stderr.is_empty());
    let report: Value =
        serde_json::from_slice(&invalid.stdout).expect("orbita should emit valid JSON");
    assert_eq!(report["valid"], false);
    assert_eq!(report["exitCode"], 1);
    assert_eq!(report["diagnostics"][0]["code"], "workflow.unresolved_node");
    assert_eq!(version.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "orbita 0.1.0"
    );
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("0.1")
        .join(name)
}
