# logika-workflow

Versioned workflow documents and pre-execution validation for Logika.

The crate decodes the `logika.dev/v1` YAML and JSON formats, reports
source-aware diagnostics, migrates supported older documents, validates graph
contracts, and compiles valid documents into immutable `ExecutionPlan` values.
Plans contain exact resolved node versions, canonical port types, deterministic
topological dependencies, an explicit execution policy, a content hash, and a
workflow-plus-lock-plus-policy cache key.

```rust
use logika_workflow::decode_yaml;

let decoded = decode_yaml(r#"
apiVersion: logika.dev/v1
kind: Workflow
metadata:
  name: empty
  version: 1.0.0
spec: {}
"#)?;

assert_eq!(decoded.document().metadata().name(), "empty");
# Ok::<(), logika_workflow::DecodeError>(())
```

Validation uses the `ValidationResolver` trait to resolve node interfaces and
canonical schemas. Compilation additionally uses `CompilationResolver` for the
exact selected versions. `logika-registry` supplies an in-memory implementation
of both contracts for local Rust nodes.

Plan compilation performs no I/O and does not execute node code. Asynchronous
execution remains the responsibility of `logika-runtime`.

See the [API documentation](https://docs.rs/logika-workflow) and the
[repository](https://github.com/aviancode/logika).

## License

MIT
