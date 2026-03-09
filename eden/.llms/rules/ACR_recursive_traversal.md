---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: 'fn blame|fn annotate|fn ancestors|fn history|fn traverse|fn walk'
---

# Recursive History Traversal

**Severity: CRITICAL**

## What to Look For

- Recursive functions that traverse commit graphs, file histories, or tree structures
- Functions where the recursion depth is proportional to the number of commits or file revisions (unbounded)
- Stack size increases as a "fix" for recursion depth issues (treats symptom, not cause)

## When to Flag

- Any function that calls itself while traversing commit ancestry, file blame/annotate, or DAG walks
- `fn foo(...) { ... foo(parent) ... }` patterns on commit/changeset/path structures
- Increasing `RUST_MIN_STACK` or thread stack size to accommodate deep recursion
- History traversal without an explicit depth limit parameter

## Do NOT Flag

- Recursive traversal of bounded tree structures (e.g., manifest trees with max depth ~20)
- Iterative implementations using an explicit stack (`Vec` as worklist)
- Recursive functions with an explicit `max_depth` parameter that is checked

## Examples

**BAD (recursive blame — matches S623056):**
```rust
fn blame_file(ctx: &CoreContext, path: &Path, cs_id: ChangesetId) -> Result<BlameResult> {
    let parent = get_parent(ctx, cs_id).await?;
    if content_changed(ctx, path, cs_id, parent).await? {
        let parent_blame = blame_file(ctx, path, parent).await?; // recurse!
        merge_blame(parent_blame, cs_id)
    } else {
        blame_file(ctx, path, parent).await? // recurse without bound!
    }
    // For files with 10K+ revisions, this exhausts the stack → SIGSEGV
}
```

**GOOD (iterative with explicit stack):**
```rust
fn blame_file(ctx: &CoreContext, path: &Path, cs_id: ChangesetId) -> Result<BlameResult> {
    let mut work_stack = vec![cs_id];
    let mut blame = BlameResult::empty();

    while let Some(current) = work_stack.pop() {
        let parent = get_parent(ctx, current).await?;
        if content_changed(ctx, path, current, parent).await? {
            blame = merge_blame(blame, current);
        }
        if parent != ROOT {
            work_stack.push(parent);
        }
    }
    Ok(blame)
}
```

**BAD (band-aid fix):**
```rust
// "Fix" for stack overflow: just increase the stack size.
// This only delays the crash for slightly deeper histories.
std::thread::Builder::new()
    .stack_size(64 * 1024 * 1024) // 64MB stack
    .spawn(move || blame_file(ctx, path, cs_id))
```

## Recommendation

Convert recursive history/DAG traversal to iterative form using an explicit `Vec`-based worklist. If recursion is unavoidable, add an explicit depth limit with a clear error when exceeded. Never increase stack size as a fix for unbounded recursion — it's a time bomb. This applies to blame, annotate, log, diff across commits, and any operation proportional to repo history depth.

## Evidence

- **S623056**: SCS server crashes (SIGSEGV) from blame algorithm using recursion proportional to file history depth. Initial fix D93493894 increased stack size, but was insufficient. Real fix D93496171 converted to iterative algorithm.
