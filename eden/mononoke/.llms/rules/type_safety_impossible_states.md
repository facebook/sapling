---
name: type-safety-impossible-states
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/.*\.rs$'
  apply_to_content: 'struct |bool|Option<|Arc<AtomicBool>|is_.*:.*bool|enum '
---

# Make Impossible States Unrepresentable

**Severity: HIGH**

## What to Look For

- Structs with multiple boolean or `Option` fields that can be in contradictory combinations
- Structs where certain field combinations are invalid but only enforced by runtime checks (asserts, panics, comments saying "caller must ensure...")
- Enums with a catch-all variant (e.g., `Unknown`) when specific variants would be more correct
- Functions with contracts like "caller should ensure X" instead of types that enforce X
- Multiple `Option` fields where at least one must be `Some`, or where they are mutually exclusive

## When to Flag

- A struct with two or more `bool` fields where some combinations are logically invalid (e.g., `immediate_persist: bool` + `no_access_to_inner: bool` where both `true` is illegal)
- A struct with `Option<A>` and `Option<B>` where exactly one must be `Some` -- should be an enum
- Runtime checks like `assert!(!(a && b))` or `if a && b { panic!(...) }` guarding struct invariants that could be enforced by the type system
- Methods with doc comments like "Caller should ensure that..." or "Must only be called when..." -- the constraint should be a type, not a comment
- A struct with a `kind` or `type_` field that determines which other fields are valid -- should be an enum with per-variant fields

## Do NOT Flag

- Structs where all field combinations are valid
- Builder patterns where validation happens in `.build()` and returns `Result`
- Protobuf/Thrift-generated types (they have their own constraints)
- Newtypes wrapping a single field
- Test code

## Examples

**BAD (contradictory state possible -- matches D32595362 review):**
```rust
struct MemWritesBlobstore {
    inner: Arc<dyn Blobstore>,
    cache: HashMap<String, Bytes>,
    no_access_to_inner: bool,
    immediate_persist: bool,
    // BUG: no_access_to_inner=true + immediate_persist=true is illegal
    // but nothing prevents constructing this state
}
```

**GOOD (enum makes states explicit):**
```rust
enum Persistence {
    Immediate,
    Delayed { access_to_inner: Arc<AtomicBool> },
}

struct MemWritesBlobstore {
    inner: Arc<dyn Blobstore>,
    cache: HashMap<String, Bytes>,
    persistence: Persistence,
}
```

**BAD (mutually exclusive options -- matches D75820141 review):**
```rust
struct DiffPathContext {
    base: Option<ChangesetPathContentContext>,
    other: Option<ChangesetPathContentContext>,
    copy_info: CopyInfo,
    // (None, None) is invalid but representable
    // (Some, None) with CopyInfo::Copy is invalid but representable
}
```

**GOOD (enum represents valid states only):**
```rust
enum DiffPathContext {
    Added(ChangesetPathContentContext),
    Removed(ChangesetPathContentContext),
    Changed {
        base: ChangesetPathContentContext,
        other: ChangesetPathContentContext,
        copy_info: CopyInfo,
    },
}
```

**BAD (comment-enforced contract):**
```rust
/// Caller should ensure that `stop()` has been called before
/// calling `try_close()`.
fn try_close(&self) -> bool {
    // ...
}
```

**GOOD (type-enforced contract -- matches D36449077 review):**
```rust
fn try_close(self) -> bool {
    // Taking `self` by value ensures the caller can't use the
    // object after closing. The type system enforces the contract.
}
```

## Recommendation

Prefer compile-time guarantees over runtime checks. This is especially important for state machines, configuration objects, and API types that cross crate boundaries in Mononoke.

## Evidence

- D32595362: Review comment -- "This feels wrong. We're allowed 3 states... Rather than error on put, can we make the three states the only three we can represent?"
- D75820141: Review comment -- "If this is an undesirable state in the struct, could we change the types to make it impossible to happen instead of relying on runtime checks?"
- D33854929: Review comment suggesting `enum Action { Exists {...}, Bookmark {...}, Change(ChangeAction) }` instead of a catch-all with runtime matching
- D36449077: Review comment -- "Having methods with contracts like 'Caller should ensure that...' is prone to errors. Instead, can we make it more ergonomic?"
