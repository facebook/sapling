# Rage sections reference

**This is a static fallback.** Prefer reading the source files directly:
- `fbcode/eden/fs/cli/rage.py` — eden rage sections (`print_diagnostic_info`)
- `fbcode/eden/scm/sapling/ext/rage.py` — sapling rage sections (`_makerage`)

If those files are inaccessible (EdenFS down, broken mount, failed clone), use this reference.

## Eden rage sections

Source: `eden/fs/cli/rage.py` → `print_diagnostic_info()`

| Section | Function | What it collects | Relevant for |
|---|---|---|---|
| System info | `print_system_info` | User, hostname, EdenFS version, RPM version, OS version, architecture | All problems — baseline context |
| Build info | `print_build_info` | EdenFS build info, daemon uptime | Crashes, version mismatches |
| Host dashboard | `print_host_dashboard` | ODS dashboard URL | Resource issues |
| EdenFS logs | `get_eden_logs` | Full edenfs.log + most recent rotated log | Crashes, errors, slow operations |
| Watchman logs | `print_watchman_log` | Watchman log files | Slow status, file state issues |
| Upgrade logs | `print_upgrade_path` | edenfs_upgrade.log | Post-upgrade failures |
| Config logs | `print_config_path` | edenfs_config.log | Config-related issues |
| Recent EdenFS logs | `print_tail_of_log_file` | Tail 20KB of edenfs.log | Quick look at recent errors |
| Running processes | `print_running_eden_process` | ps/wmic for eden processes | Crashes, multiple daemons |
| Crashes/dumps | `print_crashed_edenfs_logs` | macOS DiagnosticReports, Windows WER dumps | Crashes, OOM |
| Process tree | `print_edenfs_process_tree` | Linux ps tree for EdenFS PID | Hangs, resource issues |
| Process trace | `trace_running_edenfs` | macOS `sample`, Windows `cdb` backtrace | Hangs, deadlocks |
| Redirections | `print_eden_redirections` | Per-checkout redirect list and state | Redirection issues, build failures |
| Mount points | `print_eden_mounts` | All mounts with state, backing repo path, mount config stats | Mount issues, stale mounts, clone failures |
| Memory usage | `print_memory_usage` | `eden stats` general | OOM, high memory |
| EdenFS counters | `print_counters` (EdenFS regex) | EdenFS thrift counters | Performance, fetch issues |
| Thrift counters | `print_counters` (Thrift regex) | Thrift service counters | Thrift timeouts |
| Recent events | `print_recent_events` | `eden trace thrift/sl/inode --retroactive` | Slow operations, excessive fetches |
| PrjFS counters | `print_counters` (Windows only) | PrjFS-specific counters | Windows performance |
| EdenFS config | `print_eden_config` | `edenfsctl fsconfig --all` | Config issues, filter/sparse config, clone failures |
| Prefetch profiles | `print_prefetch_profiles_list` | Per-checkout prefetch profiles | Prefetch issues |
| VSCode extensions | `print_third_party_vscode_extensions` | Problematic extensions | Excessive file access, performance |
| Mount table | `print_system_mount_table` | System `mount` output | Stale mounts (not available on Windows) |
| Environment variables | `print_env_variables` | Host + daemon env vars | Path issues, config override |
| Disk space | `get_disk_space_usage` | `eden du --fast`, `df -h`, `diskutil` | Disk full |
| EdenFS doctor | `print_eden_doctor` | `edenfsctl doctor` output | All EdenFS issues |
| System load | `print_system_load` | `top` output | Resource exhaustion |
| Quickstack | `get_quickstack` | Stack traces | Hangs, deadlocks |
| ulimits | `print_ulimits` | `ulimit -a` | File descriptor limits |

## Sapling rage sections

Source: `eden/scm/sapling/ext/rage.py` → `_makerage()`

**Basic (always collected):**

| Section | What it collects | Relevant for |
|---|---|---|
| date, unixname, hostname | Identity | All |
| repo location, svfs location, cwd | Repo paths | Path issues |
| fstype | Filesystem type | EdenFS vs non-EdenFS |
| active bookmark | Current bookmark | Checkout state |
| hg version | Sapling version string | Version mismatches |

**Detailed (collected with timeout per section):**

| Section | What it collects | Relevant for |
|---|---|---|
| disk space usage | `df -h` or `wmic` | Disk full |
| hg sl | Smartlog with debug template | Commit graph state |
| hg debugmetalog | Metalog changes since 2d ago | Metalog corruption, lost commits |
| hg status (first 20 lines) | Working copy state | File state issues |
| hg debugmutation | Mutation history for recent drafts | Lost commits, amend/rebase issues |
| hg bookmark --list-subscriptions | Remote bookmarks | Pull/push issues |
| sigtrace | Signal trace files | Crashes |
| hg blackbox (last 500 lines) | Recent blackbox events | All — primary diagnostic source |
| hg cloud status | Commit cloud state | Cloud sync issues |
| hg debugprocesstree | Process tree | Hangs, competing commands |
| hg debugrunlog | Currently running commands | Hangs, lock contention |
| hg config (local) | Non-builtin, non-system config with sources | Config issues |
| hg sparse | Sparse profile (if sparse enabled) | Sparse/filter issues, clone failures |
| hg debugchangelog | Changelog backend info | DAG corruption |
| hg debugexpandpaths | Resolved remote paths | Path/server issues |
| hg debuginstall | Installation health check | Installation issues, missing deps |
| hg debugdetectissues | Known issue auto-detection | All — automated issue finder |
| usechg | chg enablement state | Performance |
| uptime | System uptime | Recent reboot |
| watchman debug-status | Watchman state | Slow status |
| rpm info | RPM package versions | Version mismatches, installation |
| klist | Kerberos tickets | Auth/cert issues |
| ifconfig/ipconfig | Network interfaces | Network issues |
| hg debugnetwork | Network connectivity test | Network issues |
| hg debugnetworkdoctor | Network diagnosis | Network issues |
| backedupheads | Commit cloud backup state | Cloud sync |
| commit cloud workspace sync state | Cloud sync state file | Cloud sync |
| commitcloud backup logs | Background sync logs | Cloud sync |
| scm daemon logs | SCM daemon logs | Cloud sync, background operations |
| debugstatus | Internal status state | Status issues |
| debugtree | Internal tree state | Tree issues |
| hg config (all) | Full config with sources | All config issues, filter config |
| eden rage | Full eden rage (embedded) | All EdenFS issues |
| environment variables | All env vars | Path, config override |
| ssh config | SSH config for hg server | Network, auth |
| debuglocks | Lock state | Lock contention, hangs |
| x2pagentd info | Auth proxy state | Auth issues |
| sks-agent rage | SKS agent state | Auth/cert issues |

## Problem-to-sections mapping

Use this to determine which sections to collect for a given problem class.

| Problem class | Eden rage sections to check | Sapling rage sections to check |
|---|---|---|
| **Clone failure** | System info, EdenFS config, Mount points, EdenFS doctor | hg version, hg config (all), hg sparse, hg debuginstall, rpm info |
| **Stale mount** | Mount points, Mount table, EdenFS doctor, Recent EdenFS logs | hg blackbox |
| **Slow command** | EdenFS counters, Recent events, Memory usage, Watchman logs | hg blackbox, watchman debug-status, hg debugrunlog, hg debugprocesstree |
| **Hang** | Process trace, Quickstack, Running processes, Recent events | hg debugrunlog, hg debugprocesstree, hg blackbox, debuglocks |
| **Crash / OOM** | Crashes/dumps, EdenFS logs, Build info, System info, Memory usage | hg blackbox, sigtrace |
| **Disk full** | Disk space, Mount points, Redirections | disk space usage |
| **Certificate / auth** | EdenFS config, EdenFS doctor | klist, x2pagentd info, sks-agent rage, hg debugnetwork |
| **Network** | EdenFS config | hg debugnetwork, hg debugnetworkdoctor, ifconfig, ssh config |
| **Config issue** | EdenFS config, Config logs | hg config (local), hg config (all), hg sparse |
| **Commit graph corruption** | — | hg debugchangelog, hg debugmetalog, hg debugdetectissues, hg blackbox |
| **Cloud sync** | — | hg cloud status, backedupheads, commit cloud state, scm daemon logs |
| **Filter / sparse issue** | EdenFS config, Mount points | hg config (all), hg sparse |
| **Redirection issue** | Redirections, Disk space | — |
| **Working copy corruption** | EdenFS doctor, Mount points | hg debugstatus, hg debugtree, hg blackbox |
| **Post-upgrade failure** | Upgrade logs, System info, Build info | hg version, rpm info, hg debuginstall |
| **Windows-specific** | PrjFS counters, Crashes/dumps, System info | hg config (all), environment variables |
