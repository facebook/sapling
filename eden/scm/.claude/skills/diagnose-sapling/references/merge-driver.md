# Merge Driver and Merge Tool Diagnosis

**Merge driver problems, merge tool configuration, and conflict resolution**

## How the merge driver works

The fbsource merge driver (`experimental.mergedriver = python:tools/mergedriver`) automatically regenerates checked-in generated files (CIGARs — Checked-In Generated ARtifacts) during rebase. It has two phases:

1. **Preprocess** (`preprocesshelper` in `eden/scm/sapling/fb/mergedriver/__init__.py:18`): iterates all unresolved files, calls each builder's `handlePreprocess` to check if the file is generated (`isCigar()`). If yes, marks it for driver resolution (`mergestate.mark(file, "d")`). Files in `revert_files` are reverted to trunk version.
2. **Conclude** (`concludehelper` at line 48): iterates driver-resolved files, calls `builder.doBuild()` to run the rebuild script. If `doBuild()` returns `False`, raises `MergedriverFailedException`. Successful files are marked resolved.

## How SimpleCodeGenRebuilder works

`SimpleCodeGenRebuilder` (`fbandroid/devEnv/mercurial/mergedriver/generators.py:97`, `arvr/tools/util/mergedriver/generators.py:69`) is the most common builder type. It takes:
- `cigar_directories` — directories containing generated files
- `cigar_files` — specific individual generated files
- `script` — the command to regenerate (e.g., `buck2 run xplat/instagram/settings:settings -- -s android`)
- `revert_files` — files to revert to trunk before the script runs
- Optional: `cigar_regex`, `exclude_dirs`, `exclude_files`

The `doBuild()` flow:
1. Snapshots all files in `cigar_directories` (pre-build list)
2. Sets `FILES_TO_REBUILD` environment variable with the conflicting file names
3. Runs the `script` via `repo.ui.system(cmd, blockedtag="mergedriver")` — **this `blockedtag` is why merge driver time shows up in `sl blackbox --pattern '{"blocked": "_"}'`**
4. Snapshots files again (post-build list)
5. Calls `update_mergestate_after_codegen()` to handle files added/removed by the script
6. Returns `True` on success, `False` if the script exits non-zero

## Merge driver vs merge tool — different things

- **Merge driver** (`experimental.mergedriver`): A codegen rebuilder that runs during rebase to regenerate CIGARs. Configured globally per repo. Runs scripts like `buck2 run ...`.
- **Merge tool** (`merge-tools` / `merge-patterns` config): A 3-way file merge program for resolving individual file conflicts (e.g., `vimdiff`, `kdiff3`). Configured per file pattern. `sl debugpickmergetool <file>` shows which merge tool would be used.

Users may say "merge driver" when they mean "merge tool" or vice versa. If someone says their custom merge logic for `.pmf` or `.json` files isn't working, they probably mean a **merge tool**. If they say rebase is failing/hanging/producing wrong output for generated files, they probably mean a **merge driver**.

## Diagnosing merge driver issues

```bash
# Check for merge driver blocked time in blackbox (this is how to find slow builders)
sl blackbox --start 60 --pattern '{"blocked": "_"}'
# Look for blockedtag="mergedriver" events — these show how long each builder's script ran

# Run with debug output to see which builders match and their timing
sl rebase -d <dest> --config ui.debug=true

# Check what merge driver is configured
sl config experimental.mergedriver

# Which builders are registered (look at the generators.py for your repo area)
# fbandroid: fbandroid/devEnv/mercurial/mergedriver/generators.py
# arvr: arvr/tools/util/mergedriver/generators.py

# During an unresolved merge, get context about conflicting files
sl debugconflictcontext

# Check which merge tool will be used for a specific file
sl debugpickmergetool <file>
```

**Scuba tables for merge driver/tool investigation:**
- `mergedriver_perf` — per-builder timing for merge driver runs
- `hgmergedriver_events` — merge driver events
- `dev_command_timers` — general Sapling command timing; has `fbsource_mergedriver_builders` column to correlate commands with merge drivers
- `scm_merge_conflicts` — logs manually merged files

**Important: merge driver error messages are often hidden.** When a merge driver builder fails, the Scuba trace typically shows only a generic `InterventionRequired: unresolved conflicts` error, not the actual merge driver error. To get the real error:
- Run the rebase with `--config ui.debug=true` to see full builder output
- Check the builder's script output directly
- For www merge drivers specifically, check the www merge driver Scuba table

## Common merge driver problems

- **"nested repo" error** (`path 'www/...' is inside nested repo 'www'`) — stale `www/.hg` directory from pre-MegaRepo migration. Fix: `rm -rf ~/fbsource/www/.hg`
- **Merge driver hangs during rebase** — a builder's `script` is slow. The `blockedtag="mergedriver"` means this time shows up in blackbox blocked events. Check `sl debugprocesstree` to see what subprocess the script launched. Check Scuba table `mergedriver_perf`. Workaround: disable merge driver with `sl rebase -d <dest> --config experimental.mergedriver=`
- **"Autogenerate failed" / "command_failed"** — the builder's script exited non-zero. Run with `--config ui.debug=true` to see which builder and script failed. The `FILES_TO_REBUILD` env var shows which files triggered the rebuild.
- **Silent change loss after rebase** — the `update_mergestate_after_codegen()` function compares pre/post file lists and auto-adds/removes files. If the script produces wrong output, files can be silently overwritten. Check `sl debugmutation -r <hash>` to trace the commit through the rebase, then `sl diff -r <predecessor> -r <successor>` to see what changed at each step.
- **JSON parse errors** — malformed JSON in config files used by merge drivers. Check error message for the specific file and validate JSON syntax.
- **Merge conflicts not resolving** — if mergebase is >5 days old, the www merge driver disables itself. Fix: rebase from a more recent master.
- **`isCigar` matching wrong files** — if `cigar_regex` or `cigar_directories` is too broad, non-generated files may be treated as CIGARs and overwritten by the rebuild script.

## Bypassing merge drivers

```bash
# Disable merge drivers entirely for a rebase
sl rebase -d master --config experimental.mergedriver=

# Keep original stack as backup while rebasing
sl rebase -k -d remote/fbsource/stable

# Manually resolve after skipping driver
sl resolve --all
sl rebase --continue
```

---

## Merge tools — file-level conflict resolution

Merge tools handle individual file conflicts during rebase, merge, or goto. They are different from merge drivers (see distinction above).

The default tool for interactive CLI merges is `editmerge`, which opens your editor with conflict markers.

### Internal merge tools

Built-in tools that don't require external programs:

| Tool | Behavior |
|------|----------|
| `:local` | Use the local version of the file |
| `:other` | Use the other version of the file |
| `:fail` | Don't resolve — user must run `sl resolve` |
| `:union` | 3-way merge; remaining conflicts resolved by combining both sides |
| `:merge` | 3-way merge; remaining conflicts left with merge markers |
| `:merge3` | Like `:merge` but includes a third section with base content |
| `:merge-local` | Like `:merge` but remaining conflicts resolved using local |
| `:merge-other` | Like `:merge` but remaining conflicts resolved using other |

### Merge tool selection priority

When a file has a conflict, Sapling picks a merge tool using this priority order (highest first). This is implemented in `_picktool()` in `eden/scm/sapling/filemerge.py`:

1. **`--tool` option** — command-line flag, highest priority
2. **`HGMERGE` environment variable** — can contain arbitrary shell commands
3. **`[merge-patterns]` section** — glob patterns mapping files to tools
4. **`ui.merge` / `ui.merge:interactive` config** — default tool setting
5. **`[merge-tools]` section by priority** — all registered tools, sorted by `priority` setting (higher = preferred), first one that passes validation wins
6. **`hgmerge` executable** — legacy fallback if found on PATH
7. **Internal `:merge`** — if file is not binary and not a symlink
8. **Internal `:prompt`** — absolute last resort

### Per-tool settings

Configured under `[merge-tools]` with `toolname.setting` format:

```ini
[merge-tools]
mytool.executable = /path/to/mytool
mytool.args = $local $base $other -o $output
mytool.premerge = true
mytool.priority = 10
```

| Setting | Default | Description |
|---------|---------|-------------|
| `executable` | tool name | Path to the executable |
| `args` | `$local $base $other` | Command-line args. Variables: `$local`, `$base`, `$other`, `$output` |
| `premerge` | `true` (non-binary) | Attempt simple 3-way merge first. Values: `true`, `false`, `keep`, `keep-merge3`, `keep-mergediff` |
| `priority` | `0` | Selection priority (higher = preferred) |
| `binary` | `false` | Can handle binary files |
| `symlink` | `false` | Can handle symlinks |
| `gui` | `false` | Requires a GUI (skipped if no display) |
| `disabled` | `false` | Skip this tool during selection |
| `checkconflicts` | `false` | Check output for remaining conflict markers |
| `checkchanged` | `false` | Check if output was actually modified |
| `fixeol` | `false` | Normalize EOL characters in merged output |

### `[merge-patterns]` — file-specific tool mapping

```ini
[merge-patterns]
# Use :other for generated files (take destination version)
xplat/js/RKJSModules/public/xplat-react/metadata.json = internal:other

# Use a custom script for fixture files
fbcode/instagram-server/distillery/testing/fixtures/** = fixture-merger
```

Patterns are checked in config order. The first matching pattern wins.

### Merge tool scripts — conditional tool selection

Tool names can be conditional expressions using `if()` and `isabsent()`:

```bash
# If the file was deleted on the other side, accept deletion; otherwise try merge
sl rebase -r <src> -d <dest> --tool "if(isabsent(other), :other, :merge)"

# Can also be set in config
# [ui]
# merge = if(isabsent(local), :other, if(isabsent(other), :local, :merge))
```

This is useful for change/delete conflicts where one side deleted a file and the other modified it.

### The `premerge` setting

When `premerge = true` (default for non-binary files), Sapling first attempts a simple 3-way merge before invoking the external tool. If the simple merge succeeds, the tool is never called.

Special values for the conflict marker format left in the file:
- `keep-merge3` — three sections: local, base, and other
- `keep-mergediff` — diff-based conflict sections

### Diagnosing merge tool issues

```bash
# Which merge tool will be used for a specific file?
sl debugpickmergetool <file>

# With debug output to see why tools were rejected
sl debugpickmergetool --debug <file>

# Check current merge tool config
sl config merge-tools
sl config merge-patterns
sl config ui.merge

# Check effective tool with verbose config info
sl config --verbose merge-tools
```

**Common merge tool problems:**

- **Wrong tool used for a file** → check `sl debugpickmergetool <file>` to see which tool is selected and why. Check `[merge-patterns]` for conflicting patterns.
- **Custom tool not found** → verify `executable` path exists and is executable
- **Tool not handling binary files** → set `binary = true` in the tool config
- **Conflicts left in file after merge** → check if `premerge = true` is set; the premerge might be leaving markers that the tool doesn't clean up
- **User says "merge driver" but means "merge tool"** → if the issue is about specific file patterns (`.pmf`, `.json`) or per-file conflict resolution, it's a merge tool issue. If the issue is about generated files during rebase (CIGARs, autogenerate), it's a merge driver issue.
