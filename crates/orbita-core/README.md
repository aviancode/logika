# orbita-core

Core domain types and stable contracts for the Orbita workflow engine.

This crate provides:

- validated node, plugin, port, and run identifiers;
- canonical schemas and SHA-256 type fingerprints;
- payload validation against portable schemas; and
- classified public errors with machine-readable diagnostic codes.

Most applications should depend on the [`orbita`](https://crates.io/crates/orbita)
facade. Use `orbita-core` directly when implementing an adapter or another
Orbita crate.

```rust
use orbita_core::{PrimitiveType, SchemaDefinition, TypeRef};

let order = TypeRef::new(
    "acme.order",
    1,
    SchemaDefinition::primitive(PrimitiveType::String),
)?;

println!("{}", order.fingerprint());
# Ok::<(), orbita_core::Error>(())
```

See the [API documentation](https://docs.rs/orbita-core) and the
[repository](https://github.com/aviancode/orbita).

## License

MIT
