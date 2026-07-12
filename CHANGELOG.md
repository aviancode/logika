# Changelog

All notable changes to this project are documented in this file.

## [0.1.0] - 2026-07-11

### Added

- Stable domain identifiers, classified public errors, and canonical schemas
  with SHA-256 fingerprints and payload validation.
- Versioned YAML/JSON workflow documents, format migration, source diagnostics,
  and validation for graph references, ports, cardinality, types, and cycles.
- An in-memory local-node registry and object-safe in-memory workflow storage.
- A typed Rust workflow builder and `Schema` derive with compile-time port
  compatibility checks.
- The local `logika validate` command with human-readable and JSON diagnostics
  and stable exit codes.
- Release acceptance workflows covering the public facade, incompatible edge
  diagnostics, and CLI behavior.

### Release scope

Version 0.1.0 provides workflow modeling and validation. Workflow execution is
intentionally deferred to the 0.2.0 runtime milestone.
