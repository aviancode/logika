# orbita

`orbita` is the facade crate for embedding Orbita workflow modeling and
validation in Rust applications.

Version 0.1 can:

- build type-safe workflows in Rust;
- decode versioned YAML and JSON workflow documents;
- validate node references, ports, schemas, cardinality, and graph cycles;
- register local Rust node metadata; and
- store immutable workflow definitions in memory.

Workflow execution is intentionally not part of 0.1. The asynchronous runtime
is planned for 0.2.

## Using the facade

```toml
[dependencies]
orbita = { git = "https://github.com/aviancode/orbita", tag = "v0.1.0" }
```

The `orbita` package name is already owned by an unrelated project on
crates.io, so this facade cannot be published there under its current name.
The component crates remain independently publishable.

Default features expose the `core`, `workflow`, `sdk`, `registry`, and `store`
modules. Disable default features to select only the parts an application uses:

```toml
[dependencies]
orbita = { version = "0.1", default-features = false, features = ["workflow"] }
```

See the [API documentation](https://docs.rs/orbita) and the
[repository](https://github.com/aviancode/orbita) for examples and release
notes.

## License

MIT
