# Advisory replay and rediscovery ledger

## Backtest table

An expressible history is not necessarily detected by the current checker. Each filed finding is listed by its typed identity: the canonical error set its key section states (re-derived under the current checker; the migration evidence artifact (https://artifacts.internalmeta.com/a/2351750168980124) has the classification, every key, and the rendered sections), or the execution ID of an execution finding, which is never keyed. Findings T288573775, T288573788, and T293953113 ship as `decks/bug-775.json`, `decks/bug-788.json`, and `decks/bug-113.json`; `test/corpus/known/` holds each subtask's frozen synthetic reproducer, mechanically translated into the strict result vocabulary (translated synthetic evidence, never live).

| Reference | Expressible? | Typed identity (canonical error set) | Status |
|---|---|---|---|
| [T288573775](https://www.internalfb.com/tasks/t288573775) | yes | `execution.operation-timeout` (execution finding; no task key and not a subtask of T293423744) | filed; recorded timeout is a harness failure |
| [T288573788](https://www.internalfb.com/tasks/t288573788) | yes | `path:r1` {`core.input-legality` [errno=ETIMEDOUT, input-kind=write]}; the old set {`bytes.torn-read` [channel=fuse], `status.model-live-stale-status` [expected=clean, observed=M]} is a corrected false positive (an ETIMEDOUT write is indeterminate) | filed; the original RunSpec (P2504262838) is `decks/bug-788.json` |
| [T288573794](https://www.internalfb.com/tasks/t288573794) | yes | {`status.sl-fuse-stale-status` [observed=absent, producer-kinds=write]} (legacy section: `status.model-live-stale-status` [expected=M, observed=absent]) | filed; unresolved under the contracts: never reproduced live, so it ships no deck. The recorded task evidence was a model false positive; its reproducer is synthetic. A wrong clean, an unconfirmed hypothesis, produces the key, which is also T287852915's: every omission sl status and sl diff both report keys alike |
| [T288573800](https://www.internalfb.com/tasks/t288573800) | yes | none (harness failure: operation-timeout) | filed; Sapling client, not an EdenFS correctness bug |
| [T287852915](https://www.internalfb.com/tasks/t287852915) | yes | {`status.sl-fuse-stale-status` [observed=absent, producer-kinds=write]} (legacy section: [expected=M, observed=absent]); the same key as T288573794's under the contracts | rediscovered; fixed on recorded master (closed, `fixed_in` D118734763; its RunSpec index is unbound, so a recurrence is report-only) |
| [T293953113](https://www.internalfb.com/tasks/t293953113) | yes | `execution.mount-wedged` (execution finding; no task key) | filed; a normal checkout parked in `TreeInode::checkout` while an unlink of a loaded child is dispatched wedges the mount; `decks/bug-113.json` overlaps them by its `task` declaration |
| [T287885560](https://www.internalfb.com/tasks/t287885560) | yes | none: its repro rewrites every listed file, and a stream that repeats a name a checkout replaced is legal (`readdir.duplicate-entry` excuses a name some explanation can replace: `TreeInode::readdirImpl` resumes by inode number and checkout gives a replaced entry a new, higher inode) | filed; not a finding. `decks/readdir-across-checkout.json` is now the unchanged-entry control (expected accept); `test/corpus/known/T287885560` is a synthetic reproducer (a no-op checkout repeating an unchanged name) that still rejects |

The recorded subtasks still carry legacy (`eden-correctness-task-v1`) key sections; the sections rendered under the contracts are in the migration evidence.

## Calibration rule

The calibration spec must observe before it perturbs. On the pinned SUT the calibration run must record a complete `product` or `undetermined` finding of a rule in `advisory.CALIBRATION_RULES` (every byte-content rule on a FUSE, Thrift, or held-descriptor read, `fd.read-failed`, `cross-channel.byte-disagreement`, and `status.sl-fuse-stale-status`), a violated obligation with `product` or `undetermined` attribution in a property that did not raise; the projected reason is never parsed. An accept on a correctly authored calibration spec is a regression (`detection-power-regression`), and advisory replays are refused.

## Recovery check scope

fsck as a recovery check is unsound unless the overlay has no concurrent writer and there is a version-matched helper; the stack fsck reports orphans master's daemon tolerates by design - revisit only with a daemon-side --check-only Thrift call that runs under the overlay lock.
