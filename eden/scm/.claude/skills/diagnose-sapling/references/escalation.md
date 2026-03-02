# Escalation

**When to escalate, how to collect rage, and how to write a support post**

## When to escalate

Escalate when:
- `sl doctor` and `eden doctor` can't fix the issue
- The root cause is unknown after diagnostic investigation
- The problem appears to be server-side (Mononoke errors, 500s)
- The problem requires code changes to fix (regression or bug in tooling)
- The user needs immediate help and self-service options are exhausted
- Scuba data shows a widespread regression (not just this user/host) — see [scuba-tables.md](scuba-tables.md) for the narrow → broaden pattern

## Composing the support post

**When the skill determines escalation is needed, it should compose the full post content — not tell the user to write it.** The skill has all the diagnostic evidence from the investigation. Draft the post and present it to the user to review and submit.

Include:
1. **What was observed** — user's symptom, key findings from diagnosis
2. **Diagnostic evidence** — command outputs, Scuba URLs, strace snippets, blackbox events
3. **What was ruled out** — with specific evidence for each (e.g., "Network: EdenAPI timing was 75ms out of 18s total, so network is not the bottleneck")
4. **Hypothesis** — what the skill thinks is happening based on evidence
5. **Rage file** — run `sl rage` at this point and note the output path

### Post template

```
Subject: [sl <command>] <brief description>

Environment:
- Devserver: <hostname>
- sl version: <output of sl --version>
- EdenFS version: <output of eden version>

Problem: <1-2 sentence description>

Diagnostic evidence:
<Blackbox output for the relevant PID>
<Scuba URL if applicable>
<strace output if a stuck process was found>
<eden stats summary if relevant>

What was ruled out:
- Network: <evidence from metrics JSON>
- File/tree fetches: <evidence from scmstore>
- EdenFS health: <output of eden doctor>
- Repo state: <output of sl doctor>

Hypothesis: <what the skill thinks is the root cause>

Rage file: <attached>
```

## Collecting rage

```bash
# Generate rage file for the support team
sl rage
# Note the output file path — attach it to the support post
```

**`sl rage` is a collection of command outputs, not a diagnostic tool itself.** It runs 30+ commands and concatenates their output into one file.

### Relevant sections in a rage file

**For working copy problems:**
- `disk space usage` — is the disk full?
- `first 20 lines of "hg status"` — what files are modified?
- `watchman debug-status` — is watchman healthy?
- `hg blackbox` — recent command events (but only last 15 minutes; use `sl blackbox --start 1440` directly for wider window)
- `eden rage` (embedded inside) — EdenFS health, logs, GC activity, mount status

**For commit graph problems:**
- `hg sl` — smartlog showing all draft heads and their structure
- `hg debugmetalog` — metalog entries (corruption check)
- `hg debugmutation` — mutation history for recent drafts
- `hg cloud status` — commit cloud workspace state
- `hg debugchangelog` — changelog backend info
- `hg blackbox` — recent events including visibility head counts and cloud sync

**For network problems:**
- `hg debugnetwork` — latency test and upload/download bandwidth
- `hg debugnetworkdoctor` — pass/fail network health check

**Sections you can usually skip:**
- `hg config (all)` — 500+ lines, rarely useful unless checking a specific setting
- `backedupheads` — just a list of hashes
- `rpm info`, `klist`, `ifconfig`, `airport` — system-level info, rarely relevant
- `sigtrace` — signal trace, only useful for crash investigations
- `commitcloud backup logs` — verbose, only useful for cloud sync debugging

### When to run rage vs targeted commands

- **During diagnosis**: run targeted commands (`sl blackbox`, `sl doctor`, `eden doctor`, `sl debugnetwork`). They are fast, filterable, and produce focused output.
- **At escalation time**: run `sl rage` once to generate the full dump for the support team. Attach the rage file to the support post.
- **If the user already has a rage file**: read the relevant sections listed above instead of re-running commands.

## Escalation targets

- **Sapling CLI issues** — Source Control Support group
- **EdenFS issues** — EdenFS oncall (`oncall("scm_client_infra")`)
- **Mononoke / server-side issues** — Mononoke oncall (`oncall("scm_server_infra")`)
- **Merge driver issues** — check the builder's `generators.py` for oncall annotations
