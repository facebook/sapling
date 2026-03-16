---
oncalls: ['source_control']
apply_to_regex: 'eden/mononoke/(scs/if|megarepo_api/if|derived_data/if|blobstore/if)/.*\.thrift$'
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

## When to Flag

- Adding a non-`optional` field to an existing Thrift struct
- Removing or renaming a Thrift field (instead of marking `(deprecated)`)
- Changing a Thrift field's type or field number
- Adding enum variants without verifying the consumer handles `_` / unknown
- Removing a service method that still has active traffic without a deprecation period

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

**BAD (removing enum variant):**
```thrift
enum RepoState {
  ACTIVE = 0,
  // ARCHIVED = 1,  // removed — old servers still send this!
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

## Recommendation

Always add new Thrift fields as `optional`. Never remove fields or change field IDs -- deprecate them instead. When adding new enum variants, verify that all consumers have a default/unknown handler. Consider that during rollouts, old and new server versions coexist.
