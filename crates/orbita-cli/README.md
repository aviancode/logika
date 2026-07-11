# orbita-cli

Command-line validation for Orbita workflow documents.

Version 0.1 provides `orbita validate` for local YAML and JSON files. It emits
human-readable diagnostics by default and structured JSON for automation. It
never executes node code or opens a network connection.

## Installation

```console
cargo install orbita-cli
```

## Usage

```console
orbita validate workflow.yaml
orbita validate workflow.json --json
orbita validate workflow.yaml --format json
```

Exit codes are stable for scripting:

- `0`: the document is valid;
- `1`: decoding or workflow validation failed;
- `2`: command usage, file I/O, or file-format detection failed.

The empty registry used by the 0.1 standalone CLI validates document structure
and reports unresolved node or type references. Applications with local node
registries can use the `orbita` library API for fully resolved validation.

See the [repository](https://github.com/aviancode/orbita) for workflow fixtures
and release notes.

## License

MIT
