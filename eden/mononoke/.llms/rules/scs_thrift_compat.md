---
name: scs-thrift-compat
metadata:
  oncalls: ['source_control']
  strict: true
  apply_to_path: 'eden/mononoke/(scs/if|megarepo_api/if|derived_data/if|blobstore/if)/.*\.thrift$'
  apply_to_content: 'struct |enum |service |optional|required|deprecated'
---

# SCS Thrift Backward Compatibility

**Severity: CRITICAL**

## What to Look For

- New `required` fields added to existing Thrift structs
- Removed or renamed fields in Thrift structs or enums
- Changed field IDs in Thrift definitions
- New enum variants without unknown-variant handling on the deserializing side
- API method signature changes (parameter types, return types)
- Enum definitions that start at 0 instead of 1
- Request/response type names that don't match their method name
- Inconsistent API shape across similar methods (e.g., different input types for the same concept)

## When to Flag

- Adding a non-`optional` field to an existing Thrift struct
- Removing or renaming a Thrift field (instead of marking `(deprecated)`)
- Changing a Thrift field's type or field number
- Adding enum variants without verifying the consumer handles `_` / unknown
- Removing a service method that still has active traffic without a deprecation period
- New enums with the first variant assigned value `0` -- use `1` instead (0 is the Thrift default for omitted values, which can cause silent misinterpretation)
- Request/response types not named after their method (e.g., `Request` instead of `DiffHunksRequest`)
- Similar API methods using different type shapes for the same concept (e.g., one method takes `ChangesetId + Path` as separate fields while another uses a struct)

## Do NOT Flag

- Adding new `optional` fields to existing structs
- Removing a service method or endpoint with confirmed zero traffic (e.g., traffic dashboards show no calls)
- Adding entirely new Thrift structs or services (no existing clients)
- Changes to `.thrift` files in `test/` or `if_test/` directories
- Adding `(deprecated)` annotations to existing fields
- Internal-only Thrift definitions (not crossing server boundaries)
- Changes to EdenAPI (HTTP-based, not Thrift)

## Examples

**BAD (adding required field to existing struct):**
```thrift
struct CommitInfo {
  1: binary id,
  2: string message,
  3: i64 timestamp,   // NEW required field — old clients won't send this
}
```

**GOOD (adding optional field):**
```thrift
struct CommitInfo {
  1: binary id,
  2: string message,
  3: optional i64 timestamp,  // safe: old clients just won't send it
}
```

**BAD (enum starting at 0):**
```thrift
enum DiffType {
  UNIFIED = 0,   // BAD: 0 is the default for omitted values
  HEADERLESS = 1,
  HUNKS = 2,
}
// If a client omits the field, Thrift fills in 0 (UNIFIED) silently.
// The server can't distinguish "client chose UNIFIED" from "client didn't set it."
```

**GOOD (enum starting at 1):**
```thrift
enum DiffType {
  UNIFIED = 1,
  HEADERLESS = 2,
  HUNKS = 3,
}
// Omitted field stays 0, which is no valid variant -- clearly an error.
```

**BAD (removing enum variant):**
```thrift
enum RepoState {
  ACTIVE = 0,
  // ARCHIVED = 1,  // removed -- old servers still send this!
  DELETED = 2,
}
```

**GOOD (deprecating):**
```thrift
enum RepoState {
  ACTIVE = 0,
  ARCHIVED = 1 (deprecated = "Use DELETED instead"),
  DELETED = 2,
}
```

**BAD (inconsistent naming):**
```thrift
// Method is called "diff_hunks" but types are generically named
struct Request { ... }
struct Response { ... }
```

**GOOD (method-qualified naming):**
```thrift
struct DiffHunksRequest { ... }
struct DiffHunksResponse { ... }
```

**BAD (inconsistent API shape):**
```thrift
// unified_diff takes changeset_id + path as separate fields
struct UnifiedDiffRequest {
  1: binary changeset_id,
  2: string path,
}
// headerless_diff takes a struct for the same concept
struct HeaderlessDiffRequest {
  1: DiffInput input,
}
```

**GOOD (consistent shape via common types):**
```thrift
union DiffSingleInput {
  1: DiffInputChangesetPath changeset_path;
  2: DiffInputContent content;
}

struct UnifiedDiffRequest {
  1: DiffSingleInput base,
  2: DiffSingleInput other,
}
struct HeaderlessDiffRequest {
  1: DiffSingleInput base,
  2: DiffSingleInput other,
}
```

## Recommendation

Always add new Thrift fields as `optional`. Never remove fields or change field IDs -- deprecate them instead. Start new enums at value 1, not 0 -- since 0 is the Thrift default for omitted fields, starting at 1 lets you distinguish "explicitly set" from "not set." Name request/response types after their method (`DiffHunksRequest`, not `Request`). When multiple methods operate on the same concept (e.g., diff inputs), define a common type and reuse it for API consistency and extensibility. When adding new enum variants, verify that all consumers have a default/unknown handler. Consider that during rollouts, old and new server versions coexist.

## Evidence

- D81239755: Review comments -- "Thrift best practice is to start enums at 1. This is because 0 is the default, so if a value is omitted it may get filled in as 0." / "I'd name these response/request types after the full name of the method." / "Make the shape of the API more consistent and extensible."
- D80017297: Review comment -- "Name should match the method: `RepoMultipleCommitLookupParams`."
- D63983778: Review comment -- "Naming it `errors` makes the crate name pollute the global namespace. Better to fully qualify as `scs_server_errors`."
