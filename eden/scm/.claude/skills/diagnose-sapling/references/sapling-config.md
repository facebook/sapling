# Sapling Config

**Understanding Sapling config is essential for diagnosis.** Many issues are caused by config settings — merge drivers, extensions, network settings, EdenFS behavior — all driven by config.

## Config priority (high to low)

1. Global CLI flags (e.g. `--hidden` sets `visibility.all-heads=true`)
2. `--config` items on the command line
3. `--configfile` items on the command line
4. Repo config file (`.hg/hgrc` or `.sl/config`)
   - **GOTCHA**: In EdenFS clones (shared repos), only the working copy's config file is read when running commands there — NOT the backing repo's config. EdenFS itself reads the backing repo's config.
5. User config file (`~/.hgrc` or `~/.config/sapling/sapling.conf`)
   - **GOTCHA**: `hg` and `sl` have different lookup order — `sl` prefers modern locations, `hg` prefers `~/.hgrc`. If both exist, config changes depending on which binary runs.
6. System config (`/etc/mercurial/system.rc` or `/etc/sapling/system.conf`)
7. Dynamic config (includes remote configerator configs, varies by repo name, OS, etc.)
   - Cached per-repo at `~/local/.eden-backing-repos/<repo>/.hg/hgrc.dynamic`
   - Global cache at `~/.cache/edenscm/hgrc.dynamic`
   - Refreshed every 15 minutes in background
8. Built-in static configs (hardcoded in the binary)

## Debugging config commands

```bash
# Show a specific config value with ALL sources and overrides
sl config --verbose --debug <section.name>

# Show all config sources for all values
sl config --verbose --debug

# Tree view of all configs
sl debugconfigtree

# Show the dynamic config
sl debugdumpdynamicconfig

# Show config file locations (user, repo, system)
sl configfile

# Force refresh dynamic/remote config
sl debugrefreshconfig
```

## When to check config during diagnosis

Config issues rarely present as "config issues." Users report a symptom — a command failure, slow operation, wrong output — and config turns out to be the underlying cause. **If blackbox logs and doctor checks don't explain the behavior, check the relevant config next.** Use `sl config --verbose --debug <section.name>` to see all sources and overrides — a higher-priority config file may be silently overriding the expected value.

## Merge drivers and merge tools

Merge drivers are a common cause of long-running rebases in fbsource. They are configured via Sapling config and execute external scripts during rebase/merge operations.

Key config sections:
- `experimental.mergedriver` — the merge driver command (often runs CIGAR/SimpleBuilder rebuilders)
- `merge-tools.*` — configured merge tools for file conflicts

When diagnosing merge driver issues:
- **To check what merge driver is active**: `sl config --verbose --debug experimental.mergedriver`
- **To bypass merge driver for a stuck rebase**: `sl rebase -d <dest> --config experimental.mergedriver=` (sets it to empty, disabling it)

See [merge-driver.md](merge-driver.md) for detailed merge driver diagnosis.

## Remote config (configerator)

Within Meta, Sapling pulls remote config from `configerator/source/scm/hg/hgclientconf/hgclient.cinc`. This is cached at `~/.cache/edenscm/hgrc.remote_cache` and refreshed every 15 minutes.

To test remote config changes:
```bash
HG_TEST_REMOTE_CONFIG=configerator sl debugrefreshconfig
HG_TEST_REMOTE_CONFIG=configerator sl <command>
```

## When to look at code

Some config-driven behavior can only be fully understood by reading the source code. Look at code when:
- A config value references a script or command path (e.g., merge driver) — read the script to understand what it does
- A config section controls complex behavior (e.g., `remotefilelog`, `edenfs`) — the code shows what the config actually does
- The user asks "why does X happen" and the answer depends on how a config value is interpreted by the code
- Key code locations for config loading: `eden/scm/lib/config/loader/src/`
