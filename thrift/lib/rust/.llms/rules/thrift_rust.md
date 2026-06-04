---
description: Rules for working with the Rust Thrift runtime (fbthrift crate)
apply_to_regex: "fbcode/thrift/lib/rust/.*|xplat/thrift/lib/rust/.*"
oncalls:
  - rust_thrift
---

# Rust Thrift Runtime (fbthrift) Rules

> These rules apply when working in the Rust Thrift runtime directory. This runtime is community-supported by the rust_thrift oncall (Rust Foundation).

## Critical Constraints

1. **fbcode is the source of truth.** Edit source in `fbcode/thrift/lib/rust/`. Dirsync mirrors changes to `xplat/thrift/lib/rust/`. The fbcode target `fbcode//thrift/lib/rust:fbthrift` is the primary target.
2. **BUCK is a dirsync.** The `fbcode/thrift/lib/rust/BUCK` file header says "This is a TARGETS<->BUCK dirsync" -- the fbcode BUCK syncs to `xplat/thrift/lib/rust/BUCK`.
3. **`#![deny(warnings)]`** -- all warnings are compiler errors. Fix every warning before submitting.
4. **`#![recursion_limit = "1024"]`** -- required for deep macro expansion. Do not lower this limit.
5. **Community-supported runtime.** For architecture decisions, feature requests, or design questions, direct them to the rust_thrift oncall (Rust Foundation).

## Directory Layout

| Path | Purpose |
|------|---------|
| `src/` | Core `fbthrift` crate source (`lib.rs`, protocols, client, processor, framing) |
| `src/tests/` | In-crate unit tests (binary, compact, simplejson protocol tests) |
| `src/dep_tests/` | Integration tests with generated Thrift code |
| `any/` | Thrift Any type support subcrate |
| `annotation/` | Thrift annotation support subcrate |
| `conformance/` | Conformance test support subcrate |
| `deterministic_hash/` | Deterministic hashing subcrate |
| `dynamic/` | Dynamic Thrift values subcrate |
| `universal_name/` | Universal name resolution subcrate |
| `test/` | Additional integration tests |
| `public_autocargo/` | Open-source Cargo build configuration |
| `Cargo.toml` | Cargo manifest (OSS builds, `publish = false`) |

## Key Source Files

| File | Purpose |
|------|---------|
| `src/lib.rs` | Crate root, public re-exports, `ThriftEnum` trait |
| `src/clap.rs` | `ThriftEnumValueParser` — clap `TypedValueParser` for Thrift enums |
| `src/protocol.rs` | `Protocol`, `ProtocolReader`, `ProtocolWriter` traits |
| `src/binary_protocol.rs` | `BinaryProtocol` implementation |
| `src/compact_protocol.rs` | `CompactProtocol` implementation |
| `src/simplejson_protocol.rs` | `SimpleJsonProtocol` implementation |
| `src/serialize.rs` | `Serialize` trait |
| `src/deserialize.rs` | `Deserialize` trait |
| `src/client.rs` | `Transport`, `ClientFactory` abstractions |
| `src/processor.rs` | `ServiceProcessor`, `ThriftService` traits |
| `src/framing.rs` | `Framing`, `FramingDecoded`, `FramingEncoded` traits |

## Protocols Supported

- **Binary** (`BinaryProtocol`)
- **Compact** (`CompactProtocol`)
- **SimpleJSON** (`SimpleJsonProtocol`)

## Key Buck Targets

| Target | Description |
|--------|-------------|
| `fbcode//thrift/lib/rust:fbthrift` | Main crate (source of truth) |
| `fbsource//xplat/thrift/lib/rust:fbthrift` | Dirsync mirror |
| `fbcode//thrift/lib/rust:fbthrift-unittest` | Unit tests |

## DO

- Edit source files in `fbcode/thrift/lib/rust/`. Dirsync handles xplat.
- Fix all warnings -- `#![deny(warnings)]` makes them hard errors.
- Use `bail_err!` and `ensure_err!` macros for error handling (defined in `lib.rs`).
- Run unit tests: `buck test fbsource//xplat/thrift/lib/rust:fbthrift-unittest`.
- Run dep_tests for integration coverage: `buck2 test fbcode//thrift/lib/rust/src/dep_tests/...`.

## DON'T

- Do NOT edit files directly in `xplat/thrift/lib/rust/` -- edit in fbcode and let dirsync propagate.
- Do NOT lower `recursion_limit` below 1024.
- Do NOT introduce new `#[allow(warnings)]` directives without justification.
- Do NOT modify `Cargo.toml` or `public_autocargo/` without understanding the OSS Cargo build implications.

## Running Tests

```bash
# Unit tests (in-crate)
buck test fbsource//xplat/thrift/lib/rust:fbthrift-unittest

# Integration tests (with generated code)
buck2 test fbcode//thrift/lib/rust/src/dep_tests/...

# Subcrate tests
buck2 test fbsource//xplat/thrift/lib/rust/any/...
buck2 test fbsource//xplat/thrift/lib/rust/dynamic/...
```

## Documentation

- Rust Thrift wiki: `https://www.internalfb.com/wiki/Rust/Thrift/`
- Rust SR client guide: `https://www.internalfb.com/intern/wiki/Rust-at-meta/Thrift/Service_Router_Client/`
- Rust Thrift server guide: `https://www.internalfb.com/intern/wiki/Rust-at-meta/Thrift/Writing_a_Rust_Thrift_Server/`
- Static Docs site: `https://www.internalfb.com/intern/staticdocs/thrift/`

## NOTE TO AI AGENTS

This runtime is community-supported by the rust_thrift oncall (Rust Foundation). If a user asks about architecture decisions, feature requests, or design rationale, direct them to the rust_thrift oncall rather than guessing.

<!-- TRIBAL_KNOWLEDGE: rust_thrift team should expand this with stable vs internal subcrate API boundaries -->
<!-- TRIBAL_KNOWLEDGE: How the Rust client transport layer integrates with ServiceRouter -->
<!-- TRIBAL_KNOWLEDGE: The fbthrift_library.bzl macro and how generated Rust code integrates -->
