# Configerator Setup for Hooks

After landing the hook implementation in fbsource, you need a separate configerator diff to enable it.

## Where configs live

| File | Purpose |
|------|---------|
| `source/scm/mononoke/repos/common/hooks.cinc` | Shared hook definitions for Hg repos (fbsource, www, etc.) |
| `source/scm/mononoke/repos/common/git_hooks.cinc` | Shared hook definitions for Git repos |
| `source/scm/mononoke/repos/common/repo.cinc` | Repo config builder -- wires hooks to bookmarks |
| `configerator/structs/scm/mononoke/repos/repos.thrift` | Thrift schema defining `RawHookConfig` |

## Hook entry format

Add a `RawHookConfig` to the list returned by `get_default_hooks()` in `hooks.cinc` (Hg) or `get_default_hook_set()` in `git_hooks.cinc` (Git):

```python
RawHookConfig(
    name="your_hook_name",
    implementation="your_hook_name",  # must match the string in the Rust make_*_hook() match arm
    config_json='{"key": "value"}',   # JSON config deserialized by your hook's Config struct
    bypass_pushvar="ALLOW_SOMETHING=true",  # optional: pushvar to bypass the hook
    log_only=True,  # start with True, switch to False after validation
)
```

Key fields:
- `name`: display name shown in rejection messages
- `implementation`: must exactly match the string in `make_changeset_hook`/`make_file_hook`/`make_bookmark_hook` in `implementations.rs`. Defaults to `name` if omitted.
- `config_json`: JSON string parsed by your hook's config struct via serde
- `log_only`: when `True`, the hook logs rejections but doesn't block pushes. **Always start with `True`** to validate impact without blocking engineers. Review the logs to confirm the hook isn't rejecting legitimate pushes before switching to `False`.

## Bypass control

Most hooks allow bypass via `bypass_pushvar` (anyone can set it) or `bypass_commit_string` (a magic string in the commit message). For hooks that should **not** be freely bypassable, use `bypass_permission_group` instead:

1. Create an AMP (Access Management Platform) group for the hook
2. Set `bypass_permission_group` to that group's name in the `RawHookConfig`
3. Only members of the AMP group can bypass the hook -- everyone else is blocked with no workaround

Use this for security-critical or compliance hooks where an unrestricted bypass would defeat the purpose.

## Bookmark wiring

The `repo.cinc` builder automatically wires all hooks from `get_default_hooks()` into bookmark configs. Adding your hook to `hooks.cinc` is usually sufficient -- you don't need to manually edit bookmark associations.

## Landing order

**Critical: the implementation must fully propagate before the config lands.**

The fbsource diff (hook implementation) must land and propagate to at minimum:
- **Land service**
- **Mononoke server**
- **SCS**

This propagation can take **a week or more**. Do not land the configerator diff until propagation is confirmed.

**Seek support from the Source Control team before landing configerator changes.** Don't land the config unilaterally -- coordinate with the team to verify propagation and review the rollout plan.

## Diff summary guidance

In the fbsource implementation diff summary, mention the configerator follow-up:

> Configerator follow-up needed after this propagates to land service, Mononoke, and SCS (typically 1+ week).

If you've already created the configerator diff, reference its D-number so reviewers can see the full picture.

## Rollout sequence

1. Land the fbsource implementation diff
2. Wait for propagation to land service, Mononoke server, and SCS (1+ week)
3. Coordinate with Source Control team
4. Land configerator diff with `log_only=True`
5. Monitor hook behavior in logs
6. Follow up to set `log_only=False` once validated
