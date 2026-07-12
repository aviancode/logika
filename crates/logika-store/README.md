# logika-store

Persistence contracts and in-memory workflow storage for Logika.

Version 0.1 provides the object-safe `WorkflowStore` trait and a thread-safe
`InMemoryWorkflowStore`. Workflow definitions are immutable by name and SemVer:
inserting the same document is idempotent, while conflicting contents are
rejected.

```rust
use logika_store::{InMemoryWorkflowStore, WorkflowStore};
use logika_workflow::decode_yaml;

let document = decode_yaml(r#"
apiVersion: logika.dev/v1
kind: Workflow
metadata:
  name: empty
  version: 1.0.0
spec: {}
"#)?.into_document();

let store = InMemoryWorkflowStore::new();
store.insert(document)?;
assert_eq!(store.len()?, 1);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Run state, checkpoints, and recovery contracts are planned for later releases.

See the [API documentation](https://docs.rs/logika-store) and the
[repository](https://github.com/aviancode/logika).

## License

MIT
