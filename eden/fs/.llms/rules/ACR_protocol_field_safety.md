---
name: ACR-protocol-field-safety
metadata:
  oncalls: ['scm_client_infra']
  strict: true
  apply_to_path: 'eden/fs/.*\.(cpp|h|rs|py)$'
  apply_to_content: 'field_ref|content_id_ref|serde|CBOR|Thrift|Deserialize'
---

# Protocol Backwards Incompatibility

**Severity: HIGH**

Patterns identified from analysis of ~85 EdenFS SEVs (2023-2026). Each pattern
below has caused at least one SEV.

## When to Flag

### Protocol Backwards Incompatibility (S430619, S412931)

- Removing or making optional a previously-required field in a serde/CBOR/Thrift
  struct without ensuring ALL clients handle the missing field first
- Server-side deploy of a wire format change before client-side release ships
- Optional fields unwrapped with `*field_ref()` instead of null-checked

## Do NOT Flag

- Protocol changes that only ADD optional fields (additive changes are safe)
- Fields accessed with `.has_value()` or `if (field_ref())` before unwrap
- Test code using `TestMount` or `FakeBackingStore` (controlled environments)

## Examples

**BAD (unwrapping optional without null check — S412931):**
```cpp
auto contentId = *auxData.content_id_ref();
// Crashes when server stops populating this field
```

**GOOD (null-safe access):**
```cpp
if (auto contentId = auxData.content_id_ref()) {
  // use *contentId
} else {
  // handle missing field gracefully
}
```

## Evidence

| Pattern | SEVs | Combined Impact |
|---------|------|-----------------|
| Protocol incompatibility | S430619, S412931 | buck2/hh failures, 286+ devs |
