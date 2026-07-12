# logika-sdk

Rust SDK for defining typed Logika nodes and building portable workflows.

`WorkflowBuilder` represents connections as `Output<T>` and `Input<T>`, so Rust
rejects incompatible connections at compile time. The `Schema` derive produces
the canonical schema used by portable YAML and JSON workflows.

```rust
use logika_sdk::{Node, Schema, Version, WorkflowBuilder};

#[derive(Schema)]
#[schema(name = "acme.order", version = 1)]
struct Order {
    id: u64,
}

struct ValidateOrder;

impl Node for ValidateOrder {
    type Input = Order;
    type Output = Order;

    const NAME: &'static str = "acme.validation/validate-order";
    const VERSION: &'static str = "1.0.0";
}

let mut workflow = WorkflowBuilder::new("validate-order", Version::new(1, 0, 0));
let order = workflow.input::<Order>("order")?;
let validate = workflow.node::<ValidateOrder>("validate")?;
workflow.connect(order, validate.input());
workflow.output("validated", validate.output())?;
let document = workflow.build();

assert_eq!(document.metadata().name(), "validate-order");
# Ok::<(), logika_sdk::BuildError>(())
```

The 0.1 `Node` trait describes portable node metadata only. Asynchronous node
execution is planned for the runtime milestone.

See the [API documentation](https://docs.rs/logika-sdk) and the
[repository](https://github.com/aviancode/logika).

## License

MIT
