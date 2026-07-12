# logika-sdk-macros

Procedural macros used by `logika-sdk`.

This crate is published because `logika-sdk` depends on it, but it is not the
intended user-facing entry point. Depend on
[`logika-sdk`](https://crates.io/crates/logika-sdk) and import its re-exported
`Schema` derive instead:

```rust
use logika_sdk::Schema;

#[derive(Schema)]
#[schema(name = "acme.order", version = 1)]
struct Order {
    id: u64,
}
```

See the [API documentation](https://docs.rs/logika-sdk-macros) and the
[repository](https://github.com/aviancode/logika).

## License

MIT
