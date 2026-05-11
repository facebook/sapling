# Tracing and profiling

This page is a practical guide for Sapling developers who need to understand why a command is slow, where a call came from, or which tracing target to turn on next.

## Tracing

Sapling uses Rust's [`tracing`](https://crates.io/crates/tracing) ecosystem for native tracing. Python code is not uniformly converted to `tracing`, but some Python execution paths interact with the same tracing and profiling plumbing.

### Rust tracing

Set `SL_LOG` to enable human-readable tracing output:

```sl-shell-example
$ SL_LOG=debug sl status
$ SL_LOG=commands::run=trace sl log -r .
$ SL_LOG=info,dag=debug,commands::run=trace sl log -r .
```

`SL_LOG` uses [`tracing_subscriber::filter::EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax) syntax. The useful subset is:

- `debug`: enable a level globally.
- `commands::run=trace`: enable a level for a specific target.
- `info,dag=debug`: combine directives with commas.

Tracing targets usually look like Rust module paths. The best way to find a target is often to start broad (`SL_LOG=debug`), identify the noisy or useful targets in the output, then narrow the filter.

### Python debugging log

For Python-heavy command paths, the most useful first step is usually Sapling's global debug output:

```sl-shell-example
$ sl --debug --verbose log -r .
```

`--debug` and `--verbose` are command-line flags understood by Sapling's Python command layer and extensions. They are usually more useful than `SL_LOG` for Python-only behavior. `SL_LOG` can still affect mixed Python/native paths, for example when Python calls into Rust bindings or code that emits native tracing events.

### Backtraces at tracing points

`SL_BTLOG` prints a native backtrace when a matching tracing event or span enter/exit happens:

```sl-shell-example
$ SL_BTLOG=dag::lifecycle::create=debug sl log -r .
```

`SL_BTLOG` uses the same `EnvFilter` syntax as `SL_LOG`, but the output is much larger: every matching event or span transition prints a backtrace. Use it for questions like "who constructed this object?" or "which caller reached this tracing point?", and keep the filter narrow.

### Mixed Rust/Python backtraces

On supported build combinations, Sapling can resolve Python frames inside native backtraces. This makes profiler output and tracing backtraces more useful for mixed Rust/Python command paths: instead of seeing only a Rust binding or CPython evaluation frame, the stack can include Python function names and sources such as `static:sapling.commands:3874`.

Support depends on the OS, CPU architecture, and CPython version. In practice, this is expected to work on common OS/architecture combinations with Python 3.10 or 3.12.

If Python frame resolution is not available in the current build, native frames still work, but Python-heavy sections may show up as less informative CPython or binding frames.

## Profiling

Tracing answers "what happened here?". Profiling answers "where did the time go?". Sapling has a native sampling profiler and several Python profilers.

### Native sampling profiler

Pass `--profile` to enable the native sampling profiler:

```sl-shell-example
$ sl log -r . --profile
```

The profiler samples native stacks and, on supported builds, Python frames. It prints an ASCII summary to stderr unless `profiling.output` is configured.

A shortened output looks like this:

```text
Profiling summary:
Start  Dur | Name                         Source
    1  +21 | _start
    1  +21 | main
    1  +21 | commands::run::run_command
    2  +20  \ run                         static:sapling:46
    4  +18   | dispatch                   static:sapling.dispatch:309
   19   +3    \ log                       static:sapling.commands:3874
   19   +2     | getlogrevs               static:sapling.cmdutil:3202
   21   +1     \ show                     static:sapling.cmdutil:2043
Duration 1 unit = Sampling interval = 10ms.
```

The defaults are good for a first pass. Useful knobs from `sl help config.profiling`:

```sl-shell-example
$ sl log -r . --profile --config profiling.interval=1ms
$ sl log -r . --profile --config profiling.output=/tmp/sl-profile.txt
```

Use a shorter interval for short commands or when the default 10ms interval does not collect enough samples. Sampling profilers are approximate; treat one sample as a clue, and repeated samples as evidence.

### Python profilers

Enable a Python-only profiler with:

```sl-shell-example
$ sl log -r . \
    --config profiling.enabled-python=true \
    --config profiling.type=stat
```

Available Python profiler types:

- `stat`: statistical profiler. Best for commands that run long enough to gather meaningful samples. It can show hot paths, methods, lines, or JSON depending on `profiling.statformat`.
- `ls`: Python's built-in instrumenting profiler. Works broadly, but line reporting is tied to function start lines, which can make large functions hard to diagnose.
- `traceprof`: tracing profiler. Tracks function calls and is especially useful for tree-shaped reports of small functions called many times.

Examples:

```sl-shell-example
$ sl log -r . --config profiling.enabled-python=true --config profiling.type=ls
$ sl log -r . --config profiling.enabled-python=true --config profiling.type=stat
$ sl log -r . --config profiling.enabled-python=true --config profiling.type=traceprof
```

Check `sl help config.profiling` for output formats, limits, and filtering options.

### Reading profiler output

Sapling's native profiler summarizes sampled stacks as an ASCII tree. The tree is optimized for hot paths, so it is intentionally not the same as a fully expanded tree.

An ordinary tree might render every level with extra indentation:

```text
main
  run_command
    run
      dispatch
        log
          getlogrevs
          show
```

The profiler output is more compact:

```text
Start  Dur | Name
    1  +21 | main
    1  +21 | commands::run::run_command
    2  +20 | run
    4  +18 | dispatch
   19   +3 | log
   19   +2  \ getlogrevs
   21   +1  \ show
```

Read it as:

- `Start` is the first observed time unit for that span in the rendered tree.
- `Dur` is the span duration in sampling units, not necessarily milliseconds. The footer says how large one unit is.
- `Name` is the function or frame name. `Source` usually shows a Rust symbol, Python source, or generated/static module source.
- `|` continues a straight path through nodes with a single rendered child. This avoids excessive indentation when the profile is mostly one long hot path.
- `\` starts each rendered child when a node has multiple rendered children. If one of those children then has a single rendered child, the tree switches back to `|` at the deeper indentation level.

The important move is to follow large `Dur` values downward until the time stops concentrating in one child. That split is usually where the next investigation should start.
