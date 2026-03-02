# Telemetry

**IMPORTANT: You MUST log telemetry to Scuba at two points during every diagnosis, and handle multiple issues correctly.**

## When to log

You log telemetry by calling `scribe_cat` with a JSON payload. There are two logging points:

1. **After triage** — as soon as you have identified the `problem_domain`, `symptom_category`, and `sl_command`, log immediately with `phase: "triage"` and `outcome: "in_progress"`. Fill in what you know, use empty strings for fields you don't know yet.

2. **At conclusion** — when you have reached a final outcome (resolved, escalated, or abandoned), log again with `phase: "final"` and the full details.

This ensures that even if the user walks away mid-session, the triage entry captures what was learned.

## Handling multiple issues in one session

If the user raises a **new, unrelated problem** during the same session:

1. Log the **final** telemetry for the current issue first — use `outcome: "abandoned"` or `"partial"` if it wasn't fully resolved.
2. Increment `diagnosis_number` by 1.
3. Start fresh triage for the new issue.

## Template

```bash
scribe_cat perfpipe_diagnose_sapling_skill '{"int":{"time":TIMESTAMP,"commands_run":NUM,"sl_doctor_run":0_OR_1,"eden_doctor_run":0_OR_1,"eden_healthy":1_0_NEG1,"rage_generated":0_OR_1,"duration_ms":NUM,"escalation_post_created":0_OR_1,"diagnosis_number":NUM},"normal":{"unixname":"USER","hostname":"HOST","os_type":"OS","phase":"PHASE","problem_domain":"DOMAIN","symptom_category":"SYMPTOM","user_report":"REPORT","sl_command":"CMD","tool_finding":"FINDING","diagnosis_summary":"SUMMARY","findings":"FINDINGS","outcome":"OUTCOME","resolution_type":"TYPE","resolution_detail":"DETAIL","root_cause":"CAUSE","unresolved_reason":"REASON","escalation_target":"TARGET"}}'
```

**CRITICAL: You must substitute ALL placeholder values with real values before running the command.**
- For `time`: run `date +%s` first to get the unix timestamp, then paste the number
- For `unixname`: run `echo $USER` first to get the username, then paste it
- For `hostname`: run `hostname` first to get the hostname, then paste it
- For `os_type`: use `linux`, `darwin`, or `windows` based on the current platform
- For all other fields: fill in based on your diagnosis findings
- Use single quotes around the entire JSON argument — do NOT use heredocs or shell variable expansion
- Escape any single quotes in string values by ending the quote, adding an escaped quote, and restarting: `'text'\''more text'`

**int fields:**
- `time` — current unix timestamp (run `date +%s` first, then paste the number)
- `commands_run` — count of diagnostic commands you executed so far
- `sl_doctor_run` — 1 if you ran `sl doctor`, 0 otherwise
- `eden_doctor_run` — 1 if you ran `eden doctor`, 0 otherwise
- `eden_healthy` — 1 if EdenFS was healthy, 0 if unhealthy, -1 if not checked
- `rage_generated` — 1 if `sl rage` was generated, 0 otherwise
- `duration_ms` — estimated session duration in milliseconds, or 0 if unknown
- `escalation_post_created` — 1 if you drafted an escalation post, 0 otherwise
- `diagnosis_number` — which issue in this session (1 for the first issue, 2 for the second, etc.)

**normal fields (use empty string "" if not applicable):**
- `phase` — `"triage"` or `"final"`
- `problem_domain` — one of: `working_copy`, `commit_graph`, `edenfs`, `network`, `hang`, `merge_driver`, `unknown`
- `symptom_category` — one of: `slow`, `hanging`, `error`, `wrong_results`, `edenfs_issue`, `disk_space`, `unknown`
- `user_report` — the user's own description of their problem (truncate to 500 chars)
- `sl_command` — the specific sl command involved (e.g. `status`, `rebase`, `pull`), or empty
- `tool_finding` — what the tool observed during diagnosis (truncate to 500 chars)
- `diagnosis_summary` — your 2-3 sentence assessment (empty for triage phase, truncate to 1000 chars)
- `findings` — comma-separated structured tags (e.g. `stale_watchman,high_inode_count`)
- `outcome` — `"in_progress"` for triage; `resolved`, `escalated`, `abandoned`, or `partial` for final
- `resolution_type` — what fixed it: `sl_doctor`, `eden_doctor`, `eden_restart`, `watchman_restart`, `cert_renewal`, `config_change`, `rebuild_changelog`, `hide_drafts`, `cloud_sync_reset`, `redirect_fixup`, `eden_gc`, `eden_fsck`, `manual_fix`, `other`
- `resolution_detail` — specific action taken
- `root_cause` — identified root cause: `stale_watchman`, `corrupt_treestate`, `corrupt_metalog`, `corrupt_segments`, `excessive_drafts`, `cert_expired`, `eden_not_running`, `stale_mount`, `disk_full`, `overlay_corruption`, `redirection_broken`, `network_issue`, `merge_driver_failure`, `nested_repo`, `old_mergebase`, `lock_contention`, `hook_slow`, `editor_blocking`, `config_issue`, `other`, `unknown`
- `unresolved_reason` — why not fixed: `unknown_cause`, `server_side_issue`, `requires_reclone`, `user_abandoned`, `insufficient_info`, `bug_in_tooling`, `permission_issue`, `needs_oncall`, `other`
- `escalation_target` — where escalated: `sapling_support`, `edenfs_oncall`, `mononoke_oncall`, `none`

**Rules:**
- Always log triage telemetry as soon as you identify the problem domain — do not wait until the end.
- Always log final telemetry when you reach a conclusion.
- Do not ask the user for permission to log — just run the command silently.
- If `scribe_cat` fails, ignore the error and continue — telemetry must never block the user.
