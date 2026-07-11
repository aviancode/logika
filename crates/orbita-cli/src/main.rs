//! Command-line entry point for `orbita`.

#![forbid(unsafe_code)]

use std::{
    env,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{ExitCode, Termination},
};

use orbita_registry::NodeRegistry;
use orbita_workflow::{DecodeError, ValidationError, decode_json, decode_yaml, validate_workflow};
use serde_json::{Value, json};

const EXIT_VALID: u8 = 0;
const EXIT_INVALID: u8 = 1;
const EXIT_USAGE_OR_IO: u8 = 2;
const HELP: &str = "Usage: orbita validate <WORKFLOW> [--format <human|json>]\n\
\n\
Validate a local YAML or JSON workflow without executing it.\n\
\n\
Options:\n\
      --format <human|json>  Select diagnostic output [default: human]\n\
      --json                 Shortcut for --format json\n\
  -h, --help                 Print help\n\
  -V, --version              Print version";

fn main() -> ExitCode {
    run(env::args_os()).report()
}

fn run(arguments: impl IntoIterator<Item = OsString>) -> Outcome {
    match parse_arguments(arguments) {
        Ok(Action::Help) => {
            println!("{HELP}");
            Outcome::success()
        }
        Ok(Action::Version) => {
            println!("orbita {}", env!("CARGO_PKG_VERSION"));
            Outcome::success()
        }
        Ok(Action::Validate(options)) => validate(&options),
        Err(error) => {
            let _ = writeln!(io::stderr(), "error: {error}\n\n{HELP}");
            Outcome::usage_or_io()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputFormat {
    Human,
    Json,
}

enum InputFormat {
    Json,
    Yaml,
}

impl OutputFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "human" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "unsupported output format {value:?}; expected human or json"
            )),
        }
    }
}

enum Action {
    Help,
    Version,
    Validate(ValidateOptions),
}

struct ValidateOptions {
    path: PathBuf,
    output: OutputFormat,
}

fn parse_arguments(arguments: impl IntoIterator<Item = OsString>) -> Result<Action, String> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let Some(command) = arguments.next() else {
        return Err("a command is required".to_owned());
    };

    match command.to_str() {
        Some("-h" | "--help" | "help") => return Ok(Action::Help),
        Some("-V" | "--version") => return Ok(Action::Version),
        Some("validate") => {}
        Some(command) => return Err(format!("unknown command {command:?}")),
        None => return Err("command is not valid UTF-8".to_owned()),
    }

    let mut path = None;
    let mut output = OutputFormat::Human;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => return Ok(Action::Help),
            Some("--json") => output = OutputFormat::Json,
            Some("--format" | "--output") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| format!("{} requires a value", argument.to_string_lossy()))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| "output format is not valid UTF-8".to_owned())?;
                output = OutputFormat::parse(value)?;
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option {value:?}"));
            }
            _ if path.is_none() => path = Some(PathBuf::from(argument)),
            _ => {
                return Err(format!(
                    "unexpected argument {:?}",
                    argument.to_string_lossy()
                ));
            }
        }
    }

    let path = path.ok_or_else(|| "validate requires a workflow path".to_owned())?;
    Ok(Action::Validate(ValidateOptions { path, output }))
}

fn validate(options: &ValidateOptions) -> Outcome {
    let source = match fs::read_to_string(&options.path) {
        Ok(source) => source,
        Err(error) => {
            emit_diagnostics(
                options,
                EXIT_USAGE_OR_IO,
                vec![json!({
                    "code": "cli.io",
                    "message": format!("could not read workflow: {error}"),
                })],
            );
            return Outcome::usage_or_io();
        }
    };

    let decoded = match document_format(&options.path) {
        Ok(InputFormat::Json) => decode_json(&source),
        Ok(InputFormat::Yaml) => decode_yaml(&source),
        Err(message) => {
            emit_diagnostics(
                options,
                EXIT_USAGE_OR_IO,
                vec![json!({ "code": "cli.unsupported_format", "message": message })],
            );
            return Outcome::usage_or_io();
        }
    };

    let decoded = match decoded {
        Ok(decoded) => decoded,
        Err(error) => {
            emit_diagnostics(options, EXIT_INVALID, vec![decode_diagnostic(&error)]);
            return Outcome::invalid();
        }
    };

    let registry = NodeRegistry::new();
    match validate_workflow(decoded.document(), &registry) {
        Ok(()) => {
            emit_valid(options);
            Outcome::success()
        }
        Err(errors) => {
            let diagnostics = errors
                .diagnostics()
                .iter()
                .map(validation_diagnostic)
                .collect();
            emit_diagnostics(options, EXIT_INVALID, diagnostics);
            Outcome::invalid()
        }
    }
}

fn document_format(path: &Path) -> Result<InputFormat, String> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => Ok(InputFormat::Json),
        Some("yaml" | "yml") => Ok(InputFormat::Yaml),
        _ => Err(format!(
            "cannot determine workflow format for {}; expected a .json, .yaml, or .yml file",
            path.display()
        )),
    }
}

fn decode_diagnostic(error: &DecodeError) -> Value {
    let mut diagnostic = json!({
        "code": error.code(),
        "message": error.message(),
    });
    if let Some(path) = error.path() {
        diagnostic["path"] = json!(path);
    }
    if let Some(span) = error.span() {
        diagnostic["location"] = json!({
            "line": span.start().line(),
            "column": span.start().column(),
        });
    }
    diagnostic
}

fn validation_diagnostic(error: &ValidationError) -> Value {
    let mut diagnostic = json!({
        "code": error.code(),
        "message": error.message(),
        "path": error.path(),
    });
    if let Some(node) = error.node() {
        diagnostic["node"] = json!(node.as_str());
    }
    if let Some(port) = error.port() {
        diagnostic["port"] = json!(port.as_str());
    }
    if let Some(source_type) = error.source_type() {
        diagnostic["sourceType"] = json!(type_label(source_type));
    }
    if let Some(target_type) = error.target_type() {
        diagnostic["targetType"] = json!(type_label(target_type));
    }
    diagnostic
}

fn type_label(type_ref: &orbita_core::TypeRef) -> String {
    format!(
        "{}@{}#{}",
        type_ref.name(),
        type_ref.version(),
        type_ref.fingerprint()
    )
}

fn emit_valid(options: &ValidateOptions) {
    match options.output {
        OutputFormat::Human => println!("workflow {} is valid", options.path.display()),
        OutputFormat::Json => println!(
            "{}",
            json!({
                "valid": true,
                "file": options.path.display().to_string(),
                "diagnostics": [],
            })
        ),
    }
}

fn emit_diagnostics(options: &ValidateOptions, exit_code: u8, diagnostics: Vec<Value>) {
    match options.output {
        OutputFormat::Human => {
            let mut stderr = io::stderr().lock();
            for diagnostic in &diagnostics {
                let code = diagnostic["code"].as_str().unwrap_or("cli.error");
                let message = diagnostic["message"].as_str().unwrap_or("unknown error");
                let _ = writeln!(stderr, "error[{code}]: {message}");
                if let Some(path) = diagnostic["path"].as_str() {
                    let _ = writeln!(stderr, "  at {path}");
                }
                if let Some(location) = diagnostic["location"].as_object() {
                    let line = location.get("line").and_then(Value::as_u64).unwrap_or(0);
                    let column = location.get("column").and_then(Value::as_u64).unwrap_or(0);
                    let _ = writeln!(stderr, "  --> {}:{line}:{column}", options.path.display());
                }
            }
        }
        OutputFormat::Json => println!(
            "{}",
            json!({
                "valid": false,
                "file": options.path.display().to_string(),
                "exitCode": exit_code,
                "diagnostics": diagnostics,
            })
        ),
    }
}

struct Outcome(u8);

impl Outcome {
    const fn success() -> Self {
        Self(EXIT_VALID)
    }

    const fn invalid() -> Self {
        Self(EXIT_INVALID)
    }

    const fn usage_or_io() -> Self {
        Self(EXIT_USAGE_OR_IO)
    }
}

impl Termination for Outcome {
    fn report(self) -> ExitCode {
        ExitCode::from(self.0)
    }
}
