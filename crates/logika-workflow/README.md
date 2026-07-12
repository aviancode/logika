# logika-workflow

Versioned workflow documents and pre-execution validation for Logika.

The crate decodes the `logika.dev/v1` YAML and JSON formats, reports
source-aware diagnostics, migrates supported older documents, and validates
graph references, port contracts, type compatibility, connection cardinality,
and unsupported cycles.

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
canonical schemas. `logika-registry` supplies an in-memory implementation for
local Rust nodes.

Version 0.1 validates workflows but does not execute them.

See the [API documentation](https://docs.rs/logika-workflow) and the
[repository](https://github.com/aviancode/logika).

## License

MIT
