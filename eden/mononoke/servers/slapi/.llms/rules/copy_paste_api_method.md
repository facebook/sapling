---
name: copy-paste-api-method
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/servers/slapi/.*\.rs$'
  apply_to_content: 'API_METHOD|SaplingRemoteApiMethod'
---

# Copy-Pasted API_METHOD in New Handlers

**Severity: HIGH**

## What to Look For

New handlers created by copying an existing handler often keep the old `API_METHOD` constant, causing metrics and logging to be attributed to the wrong endpoint.

## When to Flag

- A handler where `API_METHOD` doesn't match the handler's name or `ENDPOINT`
- Two different handler structs with the same `API_METHOD` value

## Do NOT Flag

- Handlers where the method name and endpoint clearly correspond

## Examples

**BAD (wrong API_METHOD from copy-paste -- matches D90503346):**
```rust
pub struct StreamingCloneHandler;

impl SaplingRemoteApiHandler for StreamingCloneHandler {
    // Copied from FilesHandler, forgot to update:
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::Files2;
    const ENDPOINT: &'static str = "/streaming_clone";
    // All streaming clone requests logged as "Files2" in ODS and Scuba
}
```

**GOOD:**
```rust
const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::StreamingClone;
const ENDPOINT: &'static str = "/streaming_clone";
```

## Evidence

- **D90503346**: StreamingCloneHandler had `API_METHOD = Files2`. All metrics were silently misattributed.
