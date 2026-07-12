//! End-to-end tests for `logika validate` output and exit codes.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn valid_yaml_uses_exit_code_zero_and_human_output() {
    let workflow = Fixture::new(
        "yaml",
        r#"apiVersion: logika.dev/v1
kind: Workflow
metadata:
  name: empty
  version: 1.0.0
spec: {}
"#,
    );

    let output = logika(["validate".as_ref(), workflow.path().as_os_str()]);

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("is valid"));
    assert!(output.stderr.is_empty());
}

#[test]
fn invalid_workflow_uses_exit_code_one_and_json_diagnostics() {
    let workflow = Fixture::new(
        "json",
        r#"{
            "apiVersion": "logika.dev/v1",
            "kind": "Workflow",
            "metadata": { "name": "unresolved", "version": "1.0.0" },
            "spec": {
                "nodes": [{ "id": "missing", "uses": "acme/missing@^1" }]
            }
        }"#,
    );

    let output = logika([
        "validate".as_ref(),
        workflow.path().as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let Ok(report): Result<Value, _> = serde_json::from_slice(&output.stdout) else {
        panic!("logika did not emit a JSON report");
    };
    assert_eq!(report["valid"], false);
    assert_eq!(report["exitCode"], 1);
    assert_eq!(report["diagnostics"][0]["code"], "workflow.unresolved_node");
    assert_eq!(report["diagnostics"][0]["node"], "missing");
    assert_eq!(report["diagnostics"][0]["path"], "spec.nodes[0].uses");
}

#[test]
fn malformed_yaml_uses_exit_code_one_and_source_location() {
    let workflow = Fixture::new(
        "yaml",
        "apiVersion: logika.dev/v1\nkind: Workflow\nmetadata: [\n",
    );

    let output = logika(["validate".as_ref(), workflow.path().as_os_str()]);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error[workflow."));
    assert!(stderr.contains("-->"));
}

#[test]
fn unreadable_path_and_bad_invocation_use_exit_code_two() {
    let missing = std::env::temp_dir().join(format!(
        "logika-cli-missing-{}-{}.yaml",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    let missing_output = logika(["validate".as_ref(), missing.as_os_str()]);
    let usage_output = logika(["unknown".as_ref()]);

    assert_eq!(missing_output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing_output.stderr).contains("error[cli.io]"));
    assert_eq!(usage_output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&usage_output.stderr).contains("unknown command"));
}

fn logika<const N: usize>(arguments: [&std::ffi::OsStr; N]) -> Output {
    let result = Command::new(env!("CARGO_BIN_EXE_logika"))
        .args(arguments)
        .output();
    let Ok(output) = result else {
        panic!("logika process did not start");
    };
    output
}

struct Fixture {
    path: PathBuf,
}

impl Fixture {
    fn new(extension: &str, contents: &str) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "logika-cli-{}-{sequence}.{extension}",
            std::process::id()
        ));
        let Ok(()) = fs::write(&path, contents) else {
            panic!("fixture could not be written");
        };
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
