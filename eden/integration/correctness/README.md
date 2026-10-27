# EdenFS exploratory correctness harness

Seeded, replayable adversarial harness for EdenFS. Generated RunSpecs
schedule filesystem operations around fault checkpoints; the checker
verdicts the durable history prefix. One integer seed determines each run,
and every run leaves enough input behind to replay the same schedule.

Generated RunSpecs and campaigns never contain a daemon kill or restart;
judging a planned kill and restart is follow-up work outside this harness.

The guarantees under test are registered rules (`RuleDef`: ID, statement,
subject kind, identity roles, red controls): the core rules in
`exploratory/core/checker.py` and the EdenFS properties in
`exploratory/edenfs/properties/`, registered in order by `PROPERTIES` in
`exploratory/edenfs/target.py`. A registered rule is asserted; there is no
maturity or evidence taxonomy and no prose contract file.
`findings_keying_test` pins the registry's identity digest; see
`exploratory/README.md`.

## The `explore` binary

All entry points hang off one par:

```sh
buck2 run fbcode//eden/integration/correctness/exploratory:explore -- <verb> ...
```

Every verb that generates, validates, runs, or checks a spec takes a
required `--target edenfs|fake`: `fake` is the in-memory target (no daemon), and
`edenfs` runs against a real daemon.

- `generate --target T --seed N [--files F]` prints one generated RunSpec
  as JSON.
- `validate --target T --spec PATH` rejects malformed RunSpecs (nonzero on
  invalid).
- `run --target T --spec PATH --artifacts DIR` runs one spec under
  containment and writes `run-spec.json`, `history.jsonl`,
  `execution.json`, `verdict.json`, `replay.json`, `admission.json`, and
  `stderr.log` into `DIR`. Exits
  nonzero unless the run completed and the verdict accepts.
- `check --target T --spec PATH --history PATH` (or `--replay PATH`)
  recomputes the offline verdict without spawning anything; exits nonzero
  unless it accepts.
- `explore --target T --seeds N [--seed-start S] [--files F] --artifacts DIR`
  generates and runs a seed range, one directory per seed plus a
  `summary.jsonl` rollup.

End to end, offline (no daemon needed):

```sh
D=$(mktemp -d)
buck2 run fbcode//eden/integration/correctness/exploratory:explore -- \
  generate --target fake --seed 8 --files 32 > "$D/seed-8.json"
buck2 run fbcode//eden/integration/correctness/exploratory:explore -- \
  validate --target fake --spec "$D/seed-8.json"
buck2 run fbcode//eden/integration/correctness/exploratory:explore -- \
  run --target fake --spec "$D/seed-8.json" --artifacts "$D/seed-8-run"
buck2 run fbcode//eden/integration/correctness/exploratory:explore -- \
  check --target fake --spec "$D/seed-8.json" --history "$D/seed-8-run/history.jsonl"
```

## Live runs

Pass `--target edenfs` to `run` or `explore` to run against a real daemon
(`generate --target edenfs` only prints an EdenFS RunSpec). The runner
process builds the Environment and runs the live
backend while the supervisor enforces the runner deadline by SIGKILL, so a dead
runner still leaves a replayable history prefix. The `edenfs` target needs a
FUSE-capable Linux host; the runner locates the daemon via
`EDENFS_SERVER_PATH` when set, else the buck-built or system paths.

```sh
buck2 run fbcode//eden/integration/correctness/exploratory:explore -- \
  explore --target edenfs --seeds 2 --seed-start 0 --files 32 \
  --artifacts "$(mktemp -d)/explore-live"
```

All daemon state and mounts are created under a dedicated `/tmp`
directory. The harness never attaches to or modifies the host's real
EdenFS daemon.
