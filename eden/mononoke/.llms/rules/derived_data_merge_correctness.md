---
name: derived-data-merge-correctness
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_path: 'eden/mononoke/.*\.rs$'
  apply_to_content: 'BonsaiDerivable|derive_single|merge.*reuse|parents.*disagree|first.parent|Reuse.*entry|content_id.*file_type'
---

# Derived Data Merge Correctness

**Severity: HIGH**

## What to Look For

- Derived data merge logic that reuses a parent's entry without verifying content
- Assumptions about which parent "wins" in a merge without documenting the convention
- Merge nodes created unnecessarily when the file was only changed on one branch

## When to Flag

- Reusing a parent's derived data entry (as an optimization to avoid creating a new blob) without verifying that the entry's content actually matches what the merge commit produces. For file-like entries, this means checking `content_id` and `file_type` match. The parent's blob may have been created with different content on a different branch.
- Merge resolution logic that doesn't document which parent's data is used when parents disagree and there's no bonsai change. The Mononoke convention is that the first parent wins, but this must be explicit in the code path, not assumed implicitly.
- Creating a new merge node (with multiple parents) for a file that has identical content in the merge commit and one of its parents. This pollutes history with unnecessary merge nodes. Instead, detect when the merge result matches a parent and reuse that parent's entry.

## Do NOT Flag

- Code that creates new entries for files explicitly changed in the bonsai changeset (these always need new entries)
- Merge logic where all parents agree (reuse is always safe when entries are identical)
- Test code

## Examples

**BAD (reuse without content check):**
```rust
// Merge: reuse first parent's entry
if parent_file.parents == new_parents {
    return Ok(parent_entry.clone()); // BUG: didn't check content_id/file_type!
}
```

**GOOD (verify content before reuse):**
```rust
// Only reuse if the content actually matches
if parent_file.content_id == content_id
    && parent_file.file_type == file_type
    && parent_file.parents == new_parents
{
    return Ok(parent_entry.clone());
}
```

**BAD (implicit first-parent-wins):**
```rust
// No bonsai change, parents disagree -- just pick one
let entry = parent_entries[0].1.clone(); // Why index 0? Not documented.
```

**GOOD (explicit convention):**
```rust
// Mononoke convention: when parents disagree and there's no bonsai
// change, the merge result matches the first parent.
let entry = parent_entries[0].1.clone();
```

## Recommendation

Always verify content fields match before reusing a parent entry in merge resolution. Document the first-parent-wins convention explicitly at each code path where it applies. Look for opportunities to reuse parent entries when a merge didn't actually change a file (content matches a parent) to avoid unnecessary merge nodes in history.
