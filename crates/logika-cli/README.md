# logika-cli

Command-line validation for Logika workflow documents.

Version 0.1 provides `logika validate` for local YAML and JSON files. It emits
human-readable diagnostics by default and structured JSON for automation. It
never executes node code or opens a network connection.

## Installation

```console
cargo install logika-cli
```

## Usage

```console
logika validate workflow.yaml
logika validate workflow.json --json
logika validate workflow.yaml --format json
```

Exit codes are stable for scripting:

- `0`: the document is valid;
- `1`: decoding or workflow validation failed;
- `2`: command usage, file I/O, or file-format detection failed.

The empty registry used by the 0.1 standalone CLI validates document structure
and reports unresolved node or type references. Applications with local node
registries can use the `logika` library API for fully resolved validation.

See the [repository](https://github.com/aviancode/logika) for workflow fixtures
and release notes.

## License

MIT
