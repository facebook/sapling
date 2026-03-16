---
oncalls: ['source_control']
apply_to_regex: 'eden/(scm|fs)/.*\.(rs|py|sh)$'
apply_to_content: 'merge.driver|hook|prefetch|sl diff|hg diff|sl cat|hg cat'
---

# Unscoped Hook and Merge Driver Fetch

**Severity: CRITICAL**

## What to Look For

- Merge drivers or build hooks that run `sl diff`, `sl prefetch`, `hg cat`, or similar operations between arbitrary commits without scoping to specific files
- Operations inside hooks whose data volume is proportional to the total number of changed files between two commits (which can be unbounded for cross-branch rebases)
- Missing per-request size limits when fetching file content from the source control server
- Merge drivers running synchronously inside the build process, inheriting the build's broad scope

## When to Flag

- Hook/merge driver code that runs `sl diff SRC DEST` without `--include` or path arguments
- `sl prefetch` or `hg prefetch` without explicit path scope in a hook context
- Any hook that fetches file content proportional to `number_of_changed_files(src, dest)` without a cap
- LFS fetch requests without a per-request size limit
- Merge conflict resolution code that loads all changed files into memory

## Do NOT Flag

- Merge drivers that explicitly scope to only the conflicting files
- Prefetch calls with explicit path patterns (`--include 'path:specific/dir'`)
- Size-limited fetch operations (e.g., `prefetch --max-size 100MB`)
- Test harnesses that exercise hook behavior with known-small inputs

## Examples

**BAD (unscoped diff in merge driver — matches S617619):**
```python
# merge_driver.py — runs during every rebase
def resolve_conflicts(source_rev, dest_rev):
    # Gets ALL changed files between source and dest — not just conflicting ones!
    diff_output = subprocess.check_output(["sl", "diff", "-r", source_rev, "-r", dest_rev])
    # For a cross-branch rebase this can be millions of files.
    # Each file triggers an LFS prefetch. 5x traffic spike ensued.
    for changed_file in parse_diff(diff_output):
        fetch_and_merge(changed_file)
```

**GOOD (scoped to conflicts only):**
```python
def resolve_conflicts(source_rev, dest_rev, conflict_files):
    # Only process the specific files that have merge conflicts
    for path in conflict_files:
        src_content = subprocess.check_output(["sl", "cat", "-r", source_rev, path])
        dst_content = subprocess.check_output(["sl", "cat", "-r", dest_rev, path])
        resolve_file(path, src_content, dst_content)
```

**BAD (no size limit on fetch):**
```rust
async fn fetch_file_content(repo: &Repo, cs_id: ChangesetId, path: &Path) -> Result<Bytes> {
    let content = repo.get_file_content(cs_id, path).await?;
    Ok(content) // What if this file is 24GB? OOM.
}
```

**GOOD (size-limited):**
```rust
const MAX_FETCH_SIZE: u64 = 100 * 1024 * 1024; // 100MB

async fn fetch_file_content(repo: &Repo, cs_id: ChangesetId, path: &Path) -> Result<Bytes> {
    let metadata = repo.get_file_metadata(cs_id, path).await?;
    if metadata.size > MAX_FETCH_SIZE {
        bail!("File {} is {} bytes, exceeds limit of {}", path, metadata.size, MAX_FETCH_SIZE);
    }
    repo.get_file_content(cs_id, path).await
}
```

## Recommendation

Hooks and merge drivers must explicitly scope their operations to only the files involved in the conflict or hook trigger — never diff or prefetch across the full commit range. Add per-request size limits for file content fetches (recommend 100MB default). For LFS, enforce a per-sync bandwidth budget. Test hooks with large cross-branch rebases to verify they don't cause traffic amplification.

## Evidence

- **S617619**: Merge driver executed `sl diff` between rebase source and destination without file scoping. Triggered `sl prefetch` for all changed files, causing sustained 5x LFS traffic increase and OOM-based load shedding. Recurred as S619192.
- **S552025**: Single 24GB commit fetched by multiple systems caused OOMs across LFS servers. No per-request memory/size budget.
