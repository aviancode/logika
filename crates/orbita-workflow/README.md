# orbita-workflow

Versioned workflow documents and pre-execution validation for Orbita.

The crate decodes the `orbita.dev/v1` YAML and JSON formats, reports
source-aware diagnostics, migrates supported older documents, and validates
graph references, port contracts, type compatibility, connection cardinality,
and unsupported cycles.

```rust
use orbita_workflow::decode_yaml;

let decoded = decode_yaml(r#"
apiVersion: orbita.dev/v1
kind: Workflow
metadata:
  name: empty
  version: 1.0.0
spec: {}
"#)?;

assert_eq!(decoded.document().metadata().name(), "empty");
# Ok::<(), orbita_workflow::DecodeError>(())
```

Validation uses the `ValidationResolver` trait to resolve node interfaces and
canonical schemas. `orbita-registry` supplies an in-memory implementation for
local Rust nodes.

Version 0.1 validates workflows but does not execute them.

See the [API documentation](https://docs.rs/orbita-workflow) and the
[repository](https://github.com/aviancode/orbita).

## License

MIT
