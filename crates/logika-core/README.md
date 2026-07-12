# logika-core

Core domain types and stable contracts for the Logika workflow engine.

This crate provides:

- validated node, plugin, port, and run identifiers;
- canonical schemas and SHA-256 type fingerprints;
- payload validation against portable schemas; and
- classified public errors with machine-readable diagnostic codes.

Applications that want the combined facade can use the Git-hosted
[`logika`](https://github.com/aviancode/logika/tree/main/crates/logika) crate.
Use `logika-core` directly when implementing an adapter or another Logika
crate.

```rust
use logika_core::{PrimitiveType, SchemaDefinition, TypeRef};

let order = TypeRef::new(
    "acme.order",
    1,
    SchemaDefinition::primitive(PrimitiveType::String),
)?;

println!("{}", order.fingerprint());
# Ok::<(), logika_core::Error>(())
```

See the [API documentation](https://docs.rs/logika-core) and the
[repository](https://github.com/aviancode/logika).

## License

MIT
