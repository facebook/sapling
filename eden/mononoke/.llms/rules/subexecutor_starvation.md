---
name: subexecutor-starvation
description: "Flag .buffered/.buffer_unordered/FuturesUnordered concurrency consumed across heavy awaits, starving the critical path"
metadata:
  oncalls: ['scm_server_infra']
  strict: true
  apply_to_path: 'eden/mononoke/.*\.rs$'
  apply_to_content: 'buffered|buffer_unordered|FuturesUnordered|FuturesOrdered'
  apply_to_clients: ['code_review']
---

# Subexecutor Starvation on Critical Paths

**Severity: HIGH**

A `.buffered(N)` / `.buffer_unordered(N)` / `FuturesUnordered` / `FuturesOrdered`
combinator is a *sub-executor*: its children only move when the owning task polls
it. Within a single task, fetching can never overlap consumption, so adding
concurrency where the consumer awaits real work between polls cannot speed the loop
up — it only adds contention, memory, and (for the ordered forms) head-of-line
blocking behind the first item. Worse, children competing for bounded resources
(channels, locks, pipe buffers) can deadlock in ways that look like innocent async
code with no locks held.

## When to Flag

Flag NEW code introduced by the diff when ALL of these hold:

1. It builds a sub-executor over futures that perform I/O or hold bounded
   resources: `.buffered(N)` / `.buffer_unordered(N)` with N > 1, or
   `FuturesUnordered` / `FuturesOrdered` fed with unspawned futures.
2. The children are NOT independently driven: no `spawn_task` / `tokio::spawn` /
   `JoinSet`, not `JoinHandle`s or channel receivers.
3. The stream is consumed across awaits of real work, e.g.
   `while let Some(item) = s.try_next().await { heavy_io().await; }` — any
   `.await` between polls that does storage/RPC/lock/sleep work, or that can
   block on resources the children need.
4. The loop is on a throughput- or latency-sensitive path (derivation loops,
   request handlers, tailers, backfills), or the children share bounded
   resources (deadlock risk anywhere).

Conversions of sequential `.then()` to `.buffered(N)` feeding such a loop are the
highest-signal case: `.then()` never strands work, `.buffered(N)` strands up to N.

## Do NOT Flag

- A buffered stream drained immediately with no other awaits in between:
  `.buffered(N)....try_collect().await`, `.collect().await`, `.count().await`.
- Consumers doing only cheap synchronous work between polls (push to `Vec`,
  counters, in-memory transforms, logging).
- Children that are spawned (`mononoke::spawn_task`, `tokio::spawn`,
  `JoinSet::spawn`), `JoinHandle`s, or channel receivers — the main executor
  drives them.
- `bounded_traversal` — it already spawns contained futures internally.
- Sequential consumption: `.then()`, `buffered(1)`.
- Test-only code, or pre-existing occurrences the diff doesn't touch.

## Examples

**BAD (concurrent fetch feeding a heavy loop — D122116536):**
```rust
// segments.rs: up to 100 range_stream traversals in flight ...
let slices = stream::iter(segments)
    .map(move |segment| async move {
        Ok(graph.range_stream(&ctx, segment.base, segment.head).await?
            .collect::<Vec<_>>().await)
    })
    .buffered(100)
    .boxed();
// ... consumed across derivation awaits in derive.rs:
while let Some(batch) = slices_stream.try_next().await? {
    // Siblings sit frozen, holding storage resources, during every derive.
    self.derive_exactly_batch::<Derivable>(&ctx, batch, rederivation.clone()).await?;
}
```

**GOOD (buffered + immediate drain, no awaits between polls):**
```rust
// derive.rs gap-parent lookup: try_collect does trivial work between polls.
let gap_parents: Vec<_> = stream::iter(external_parents)
    .map(|cs_id| async move { /* fetch_derived */ })
    .buffered(100)
    .try_filter_map(|(cs_id, derived)| async move {
        Ok(if derived { None } else { Some(cs_id) })
    })
    .try_collect()
    .await?;
```

**GOOD (keep it sequential when the consumer awaits heavy work):**
```rust
// One segment at a time; nothing is ever stranded during derive.
.then(move |segment| async move { /* range_stream ... */ })
```

**GOOD (spawn so the main executor drives the children):**
```rust
.map(|segment| mononoke::spawn_task(async move { /* ... */ }))
.buffered(100) // now holds JoinHandles; safe, but results may be discarded
```

## Recommendation

- Keep `.then()` (or `buffered(1)`) when each item triggers heavy awaits: in a
  single task, fetching cannot overlap consumption anyway.
- Or drain first (`.try_collect()` into a `Vec`), then loop — if memory allows.
- Or spawn children via `mononoke::spawn_task` (requires `'static`) or `JoinSet`
  so the executor drives them to completion; accept possibly wasted work.
- Lowering N dampens contention but does NOT remove deadlock risk when children
  share bounded resources; only sequential / spawned / immediately-drained does.

## Finding Content

Every finding must name the exact "real work" it claims is starved, so the
author can judge conditions 3-4 without hunting:

- The await expression between polls (e.g.
  `self.derive_exactly_batch::<Derivable>(&ctx, batch, ...).await`), with its
  repo-root-relative file path and line number.
- What kind of work it is (storage I/O, RPC, lock, sleep, ...) and why it
  blocks polling of the buffered children.
- The stranded children: the buffered combinator, its N, and what each child
  holds while frozen.
- When the stream is built in one place and consumed in another, cite both
  sites.

If the consumer or the await cannot be identified precisely, say so and do not
flag — a vague "this might starve something" is noise.

## Justification

- The "subexecutor problem" in Async Rust: https://fb.workplace.com/notes/1722678438098836/
- D122116536 regressed derivation by converting `.then()` to `.buffered(100)`
  on a stream consumed across `derive_exactly_batch` awaits.
- Precedent fix by spawning: D37863447; `bounded_traversal` CPU/deadlock
  history: https://fb.workplace.com/groups/scm.mononoke/permalink/1767698556926045/
