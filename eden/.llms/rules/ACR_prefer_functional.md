---
oncalls: ['source_control']
apply_to_regex: 'eden/(mononoke|scm)/.*\.rs$'
apply_to_content: 'let mut.*=.*new\(\)|let mut.*=.*default|let mut.*=.*String|\.push\(|for .* in .*\{.*\.push'
---

# Prefer Immutable, Expression-Oriented Code

**Severity: MEDIUM**

## What to Look For

### Unnecessary mutability

- `let mut x = default; if cond { x = val; }` — should be `let x = if ... { } else { };`
- `let mut x = a; x = b;` — just use `let x = b;` or two separate bindings
- `let mut` where the binding is only written once after declaration
- Mutable variables used as output parameters instead of returning values
- `let mut` inside a function that could return the computed value directly

### Imperative loops replaceable by expressions

- `let mut vec = Vec::new(); for x in items { vec.push(f(x)) }` — use `.map().collect()`
- `let mut acc = init; for x in items { acc = combine(acc, x) }` — use `.fold()`
- `let mut found = false; for x in items { if pred(x) { found = true; break } }` — use `.any()`
- `let mut count = 0; for x in items { if pred(x) { count += 1 } }` — use `.filter().count()`

## When to Flag

**Mutability violations (primary):**
- `let mut` followed by a single conditional reassignment — use `let x = if/match`
- `let mut` followed by a single reassignment — use two `let` bindings
- `let mut` where the value is set in every branch of a match/if — use the expression as the initializer
- Functions that take `&mut output_vec` and `.push()` into it instead of returning a `Vec`

**Imperative style (secondary):**
- Collect-into-vec loops replaceable by `.iter().map(...).collect()`
- Mutable counters/accumulators replaceable by `.filter().count()`, `.fold()`, or `.sum()`
- Search loops replaceable by `.find()`, `.any()`, `.all()`, `.position()`
- Imperative `HashMap` building when `.into_group_map()` or `.collect::<HashMap<_,_>>()` works

## Do NOT Flag

- `let mut` for builders (e.g., `MononokeAppBuilder`) — builder pattern is inherently mutable
- Mutable borrows required by APIs (`&mut self` receivers)
- `let mut stream = ...` for async streams needing `.next().await` in a `while let`
- Genuine accumulation with complex control flow (early returns, `?` mid-loop, interleaved I/O)
- Performance-critical hot paths where the imperative version is measurably faster
- Loop bodies with side effects (logging, metrics) interleaved with data accumulation

## Examples

**BAD (mutable conditional init):**
```rust
let mut prefix = String::new();
if use_repo_prefix {
    prefix = format!("{}:", repo_name);
}
```

**GOOD (expression):**
```rust
let prefix = if use_repo_prefix {
    format!("{}:", repo_name)
} else {
    String::new()
};
```

**BAD (mutable reassignment):**
```rust
let mut path = raw_path.to_string();
path = path.trim_start_matches('/').to_string();
```

**GOOD (binding chain):**
```rust
let path = raw_path.trim_start_matches('/');
```

**BAD (mutable match output):**
```rust
let mut mode = FileMode::Regular;
match entry.file_type() {
    FileType::Executable => mode = FileMode::Executable,
    FileType::Symlink => mode = FileMode::Symlink,
    _ => {}
}
```

**GOOD (match expression):**
```rust
let mode = match entry.file_type() {
    FileType::Executable => FileMode::Executable,
    FileType::Symlink => FileMode::Symlink,
    _ => FileMode::Regular,
};
```

**BAD (output parameter):**
```rust
fn collect_paths(entries: &[Entry], out: &mut Vec<MPath>) {
    for e in entries {
        out.push(e.path().clone());
    }
}
```

**GOOD (return value):**
```rust
fn collect_paths(entries: &[Entry]) -> Vec<MPath> {
    entries.iter().map(|e| e.path().clone()).collect()
}
```

**BAD (imperative accumulator):**
```rust
let mut total_size = 0u64;
for entry in entries {
    total_size += entry.size();
}
```

**GOOD (functional):**
```rust
let total_size: u64 = entries.iter().map(|e| e.size()).sum();
```

**BAD (imperative search):**
```rust
let mut has_binary = false;
for file in changed_files {
    if file.is_binary() {
        has_binary = true;
        break;
    }
}
```

**GOOD (predicate):**
```rust
let has_binary = changed_files.iter().any(|f| f.is_binary());
```

## Recommendation

Treat `let mut` as a code smell that needs justification. Rust's expression-oriented design means `if`, `match`, and blocks all return values — use that to initialize bindings immutably. When reviewing, ask in order: (1) can this `let mut` be a `let` with an expression initializer? (2) can this loop be an iterator chain? Only if both answers are "no" due to genuine complexity is `let mut` appropriate.
