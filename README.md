# Logika

Logika is an embeddable, clientless workflow engine for Rust applications. It
models workflows as typed directed graphs, validates them before execution, and
keeps ownership of storage, networking, UI, and authentication in the host
application.

Version 0.1.0 delivers the modeling and validation foundation:

- canonical schemas with stable SHA-256 type fingerprints;
- versioned YAML and JSON workflow documents;
- validation of node references, ports, types, cardinality, and graph cycles;
- a typed Rust builder that rejects incompatible ports at compile time;
- in-memory node registration and immutable workflow storage; and
- `logika validate` with human-readable or JSON diagnostics.

Workflow execution is intentionally outside the 0.1.0 scope. The asynchronous
DAG runtime, retries, timeouts, cancellation, and tracing are planned for 0.2.0.

## Quick start

Install the validator from crates.io:

```console
cargo install logika-cli
```

Create `workflow.yaml`:

```yaml
apiVersion: logika.dev/v1
kind: Workflow
metadata:
  name: empty-workflow
  version: 1.0.0
spec: {}
```

Validate it locally:

```console
logika validate workflow.yaml
logika validate workflow.yaml --json
```

Exit code `0` means the document is valid, `1` reports decoding or validation
errors, and `2` reports command usage, file I/O, or format detection errors.

For embedded use, depend on the components your application needs:

```toml
[dependencies]
logika-core = "0.1"
logika-registry = "0.1"
logika-sdk = "0.1"
logika-store = "0.1"
logika-workflow = "0.1"
```

The `logika` facade is available directly from this repository because that
package name belongs to an unrelated project on crates.io:

```toml
[dependencies]
logika = { git = "https://github.com/aviancode/logika", tag = "v0.1.0" }
```

## Workspace

| Crate | Version 0.1.0 role | Published |
| --- | --- | --- |
| `logika` | Feature-gated facade for the embedded API | Git only |
| `logika-core` | Identifiers, schemas, ports, and public errors | crates.io |
| `logika-workflow` | Document decoding, migration, and validation | crates.io |
| `logika-registry` | In-memory local node and schema registry | crates.io |
| `logika-sdk` | Typed node API and workflow builder | crates.io |
| `logika-sdk-macros` | `Schema` derive implementation | crates.io |
| `logika-store` | Storage contracts and in-memory workflow store | crates.io |
| `logika-cli` | Local `validate` command | crates.io |
| `logika-runtime` | Runtime milestone placeholder | No |
| `logika-plugin-api` | Plugin contract milestone placeholder | No |
| `logika-plugin-host` | Plugin host milestone placeholder | No |
| `logika-testkit` | Integration testkit milestone placeholder | No |

Release notes are kept in [`CHANGELOG.md`](CHANGELOG.md).

## Development

The workspace uses Rust 1.96.1 and Edition 2024. Run the same checks as CI with:

```console
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo doc --workspace --no-deps
```

## Releases

Pushing a workspace tag such as `v0.1.0` runs all CI checks and publishes the
seven crates.io packages in dependency order. The repository must provide a
`CARGO_REGISTRY_TOKEN` Actions secret with permission to publish them. The
workflow checks that the tag matches the workspace version and safely skips
versions that already exist, so a partially completed release can be rerun.

## License

Logika is licensed under the [MIT License](LICENSE).
