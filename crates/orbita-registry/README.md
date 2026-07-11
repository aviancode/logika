# orbita-registry

Deterministic in-memory node registration for Orbita.

Version 0.1 stores metadata for local Rust nodes, resolves the highest version
matching a workflow's SemVer requirement, indexes schemas declared on node
ports, and implements `orbita_workflow::ValidationResolver`.

```rust
use orbita_registry::NodeRegistry;

let registry = NodeRegistry::new();
assert!(registry.is_empty());
```

Plugin packages, remote indexes, signatures, and lock-file resolution are not
part of 0.1; they are planned for the plugin milestone.

See the [API documentation](https://docs.rs/orbita-registry) and the
[repository](https://github.com/aviancode/orbita).

## License

MIT
