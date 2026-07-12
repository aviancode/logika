# Changelog

All notable changes to this project are documented in this file.

## [0.1.2] - 2026-07-12

### Changed

- Published the `logika` facade crate as the primary crates.io entry point.
- Retried crates.io uploads with bounded backoff when the registry responds
  with HTTP 429 rate limiting.

## [0.1.1] - 2026-07-12

### Changed

- Raised the minimum supported Rust version to 1.96 and pinned local and CI
  tooling to Rust 1.96.1.
- Updated the registry implementation for the Rust 1.96 Clippy lint set.

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
