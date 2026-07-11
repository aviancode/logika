# orbita-sdk-macros

Procedural macros used by `orbita-sdk`.

This crate is published because `orbita-sdk` depends on it, but it is not the
intended user-facing entry point. Depend on
[`orbita-sdk`](https://crates.io/crates/orbita-sdk) and import its re-exported
`Schema` derive instead:

```rust
use orbita_sdk::Schema;

#[derive(Schema)]
#[schema(name = "acme.order", version = 1)]
struct Order {
    id: u64,
}
```

See the [API documentation](https://docs.rs/orbita-sdk-macros) and the
[repository](https://github.com/aviancode/orbita).

## License

MIT
