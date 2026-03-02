# EdenFS Config

**EdenFS config hierarchy, debugging, and diagnostic traps**

EdenFS has its own config system, separate from Sapling's config. Understanding it is essential because config can cause hangs, slow operations, and hard-to-diagnose behavioral differences.

## Config priority (low to high)

EdenFS config follows a 5-level priority hierarchy. Higher levels override lower levels:

1. **Default** — hardcoded defaults in the C++ code (`eden/fs/config/EdenConfig.h`)
2. **SystemConfig** — `/etc/eden/edenfs.rc` (Chef-managed, system-wide)
3. **Dynamic** — `/etc/eden/edenfs_dynamic.rc` (written by `edenfs_config_manager` every 15 minutes from Configerator rollouts)
4. **UserConfig** — `~/.edenrc` (user-specific overrides)
5. **CommandLine** — CLI flags (highest priority)

Priority is defined in `eden/fs/config/ConfigSetting.cpp:12-17`.

**Important:** The Thrift enum values in `eden_config.thrift` (`ConfigSourceType`) do NOT match priority indices — don't use enum ordinal values to infer priority.

## Best debugging command

```bash
# Show ALL config settings with their current values and sources
edenfsctl fsconfig --all
```

This is the single most useful command for EdenFS config debugging. It shows every config key, its current value, and which source level it came from (Default, SystemConfig, Dynamic, UserConfig, or CommandLine).

```bash
# Show a specific config value
edenfsctl fsconfig <section:key>

# Example
edenfsctl fsconfig fuse:request-timeout
```

## Config files

### System config: `/etc/eden/edenfs.rc`
- Chef-managed, shared across all users on the machine
- Contains: FUSE request timeouts, store size limits, SSL cert paths, hash keys
- Example settings that affect behavior:
  - `fuse:request-timeout` — on devservers, often set to `1d` (1 day!). This means FUSE operations won't time out for a full day, which can mask hangs.
  - `store:*` — cache size limits for blob, tree, and metadata caches

### Dynamic config: `/etc/eden/edenfs_dynamic.rc`
- Written by `edenfs_config_manager` every 15 minutes
- Reads from Configerator rollouts with platform/tier/GK/percentage filters
- Contains: mount settings, thrift function allowlists, experimental flags, prefetch settings
- Raw Configerator response: `/etc/eden/edenfs_dynamic_raw` (JSON with rollout expressions)

### User config: `~/.edenrc`
- Per-user overrides (rarely used, but highest file-based priority)

### Per-checkout config: `~/.eden/clients/<checkout>/config.toml`
- Mount-specific settings (backing repo path, mount protocol, redirections)

## CLI vs daemon config divergence — diagnostic trap

The EdenFS Python CLI reads config files from `/etc/eden/config.d/*.toml` (drop-in fragments like `00-defaults.toml`, `doctor.toml`, `zz-fb-chef.toml`), but the **EdenFS daemon does NOT read these files**. The daemon only reads `edenfs.rc`, `edenfs_dynamic.rc`, `~/.edenrc`, and per-checkout configs.

This means:
- `eden doctor` may use different thresholds than what the daemon enforces
- Settings in `config.d/` affect CLI behavior (doctor checks, du calculations) but not filesystem behavior
- If you see a config value in `config.d/` that should affect daemon behavior, it's NOT being applied to the daemon

**To check what the daemon actually sees:** use `edenfsctl fsconfig --all` (this queries the running daemon via Thrift).

## Config settings that can cause hangs

These settings directly affect timeout behavior and can cause commands to appear hung:

- **`fuse:request-timeout`** — default: 1 minute. On devservers often set to `1d` via system config. Controls how long the kernel waits for EdenFS to respond to a FUSE request before returning EIO.
- **`nfs:request-timeout`** — similar timeout for NFS mounts
- **`store:*` cache sizes** — if caches are too small, excessive eviction causes repeated fetches
- **`thrift:*` settings** — thrift request queue sizes and timeouts

## Dynamic config system (Configerator)

The `edenfs_config_manager` service runs every 15 minutes and:
1. Reads rollout configurations from Configerator
2. Evaluates platform/tier/GK/QE/percentage filters for the current machine
3. Writes the resolved config to `/etc/eden/edenfs_dynamic.rc`

The daemon picks up changes from `edenfs_dynamic.rc` on its own refresh cycle (also ~15 minutes). So a Configerator change can take up to 30 minutes to take effect.

To see the raw rollout expressions: `cat /etc/eden/edenfs_dynamic_raw`

## Diagnostic checklist

When config is suspected:
1. Run `edenfsctl fsconfig --all` to see what the daemon actually uses
2. Check `/etc/eden/edenfs.rc` for system-level overrides (especially timeouts)
3. Check `/etc/eden/edenfs_dynamic.rc` for dynamic settings
4. Check `~/.edenrc` for user overrides
5. Compare CLI behavior (which reads `config.d/`) with daemon behavior (which doesn't)
