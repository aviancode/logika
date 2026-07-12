# logika

`logika` is the facade crate for embedding Logika workflow modeling and
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
logika = "0.1.2"
```

The facade and its component crates are published on crates.io.

Default features expose the `core`, `workflow`, `sdk`, `registry`, and `store`
modules. Disable default features to select only the parts an application uses:

```toml
[dependencies]
logika = { version = "0.1.2", default-features = false, features = ["workflow"] }
```

See the [repository](https://github.com/aviancode/logika) for examples and
release notes.

## License

MIT
