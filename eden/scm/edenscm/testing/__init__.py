# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

r"""
The `testing` module provides support to run integration tests in .t format
without depending on external POSIX binaries such as `/bin/bash`.


Usage
=====

Via `hg debugruntest`:

    hg debugruntest test-foo.t test-bar.t ...

Via `run-tests.py`. Only tests with `#debugruntest-compatibile` will run
through this `testing` module:

    run-tests.py test-foo.t test-bar.t ...

Differnce between `hg debugruntest` and `run-tests.py`:
- `run-tests.py` supports tests that require a real `bash`. Those tests
  might use `#require bash` so they can be skipped by `debugruntest`.
  See "Compatibility" below.
- `hg debugruntest` prints output mismatch per check in a streaming way,
  using hg's own diff format (with word diff coloring). `run-tests.py`
  prints mismatches per test file (diff between `.t` and `.t.err`).
- Both support `-i` for updating outputs. `hg debugruntest` does not
  show interactive prompts. Use `hg revert -i` afterwards to interactively
  select updated outputs.
- `hg debugruntest` can also run or update Python doctests.
- `hg debugruntest` supports extended .t syntax. See below.


Compatibility
=============

Tests passing with `run-tests.py` and a real `bash` can fail with
`hg debugruntest` in a few ways:

- Shell syntax.
  `testing.sh` could behave differently for minor things like escaping in
  heredoc, etc. Usually there is a way to change the code slightly to be
  compatible with `debugruntest` or both test runners.

- Shell builtins and coreutils.
  `testing.sh.stdlib` emulates a subset of shell builtins and coreutils in
  Python. This emulation is not complete and not intended to be full
  POSIX-complaint. Depending on the issue, it might make sense to extend the
  emulation or choose an alternative way to do things.

- External binaries.
  The shell emulation avoids implicit dependencies on external binaries by
  reseting `PATH` to not contain common paths like `/bin`. External binaries
  need to be explicitly `#require`-ed so they become available in both shell
  emulation and `PATH`.

- In-process `hg` and Python global side effects.
  For performance, the `testing.ext.hg` registers the shell command `hg` as an
  in-process function.  `uisetup()` per extension only runs once because
  "loaded" extensions won't be loaded again. Side effects in `uisetup()` could
  be unintentionally permanent (cannot disable by disabling the extension) or
  unintentionally lost (if uisetup() changes the `ui` that will be recreated
  for the next command).  Sometimes it's possible to track down the side
  effects and fix it by gating with config options. If the fix is non-obvious,
  `#inprocess-hg-incompatible` can make a test run `hg` as external process -
  trade performance for compatibility. `#inprocess-hg-incompatible` can be used
  together with `#chg-compatibile` to restore some performance using chg.

- Python block `>>>` behavior is different.
  In `debugruntest` the block won't be treated as a shell heredoc so things
  like `$TESTTMP` won't be substituted. Like Python doctest, `debugruntest`
  uses `compile(mode='single')` so non-None expressions will be printed.
  Python side effects made by the `>>>` block in `debugruntest` affects the
  rest of test file.

- Output check is slightly different.
  In `debugruntest` trailing blank lines are ignored. This could make
  `hg log -G` tests cleaner.


Extended .t syntax
==================

- Indented blocks.
  `$` and `>>>` blocks can have more than 2 spaces indentation.

- Hybrid Python block.
  Python block can be used without `>>>`. In the Python block regular
  `$` and `>>>` blocks can be used. This can be helpful to express
  things that are harder for bash.
  Consider only use this feature when bash or Python doctest sucks, since it
  is incompatible with `run-tests.py` and `$` and `>>>` blocks are much easier
  to parse and codemod.
  If unsure about how this works, check `__pycache__/ttest/test-name.py` for
  generated Python code transformed from `.t` source.


Internals
=========

Moudles
-------

`testing` has a few modules:

- `sh`: bash interpreter
- `t`: `.t` runner
- `ext`: extensions for optional setup in `.t` tests


Bash interpreter
----------------

The bash interpreter uses `conch_parser` to convert bash code to AST and
interprets the AST. It tries to abstract real OS side effects away (ex. do
not use `os.environ` or `os.chdir`, or `open` directly unless told).

`types.Env` defines the "environment" state to run scripts. `stdlib` contains
shell builtins and common utilities.

`types.ShellFS` abstracts the filesystem. `OSFS` integrates with a real FS.
`TestFS` keeps files in-memory for side-effect-free testing.

If you modify the shell interpreter or the stdlib, consider adding a doctest
to `__init__.py`.


.t runner
---------

There are multiple steps to run a `.t` test:
- Translate `.t` to Python code. This is mainly done in `transform.py`.
  The translated result is written to `__pycache__/ttest/<name>.py` for easier
  debugging.
- Setup test environment. This is done by `runtime.TestTmp`. See its docstring
  for how to add new shell functions or affect the shell environemnts.
- Extends `TestTmp` for optional features. See "Extensions" below.
- Compare output. Output cannot be compared directly. For example, the test
  path is substituted to `$TESTTMP`, and Certain text like `(glob)`, `(?)`,
  `(no-windows !)` have special meanings. Text substitution is done by
  `TestTmp`. Special text is handled by `diff.py`.
- Feature detection for things like `#require symlink`. This is currently
  handled in `hghave.py`.
- Auto-fix. This is done by `runner.fixmismatches`.

`runner.py` contains logic to put things together.

`runner.TestRunner` manages multiple processes to run tests (so each test has a
clean Python state from start), and streams test events (ex. single output
mismatch, exception, test file pass) as Python objects to its caller.
`TestRunner` does not print anything and it's up to the caller to decide how to
output. Python stdlib `multiprocessing` provides IPC - child and parent process
communicate using Python objects. The call graph looks like:

    debugruntest                (pid 100, handle events and print to stdout)
      \_ runner.TestRunner      (pid 100, stream test events without output)
          |- runner._spawnmain  (pid 101, collect test result and report)
          |   \_ runner.runtest (pid 101, run test-1.t)
          |- runner._spawnmain  (pid 102, collect test result and report)
          |   \_ runner.runtest (pid 102, run test-2.t)
          :                     (more processes to run tests in parallel)

When using `run-tests.py`, child process management is done by `run-tests.py`
instead of `runner.TestRunner`. IPC uses CLI args, exit code, and filesystem
(write `.err` files). The call/process graph looks like:

    run-tests.py                (pid 100, tid 100)
      |- DebugRunTestTest.run   (pid 100, tid 101)
      |  \_ python -m edenscm.testing.single test-1.t -o test-1.t.err (pid 101)
      |      \_ runner.runtest  (pid 101, run test-1.t)
      |- DebugRunTestTest.run   (pid 100, tid 102)
      |  \_ python -m edenscm.testing.single test-2.t -o test-2.t.err (pid 102)
      |      \_ runner.runtest  (pid 102, run test-2.t)
      :                         (more processes to run tests in parallel)


Extensions
----------

EdenSCM tests were the main motivation to create `testing`. But the general
`.t` format seems useful beyond just EdenSCM use-cases. So the shell
interpreter and the `.t` runner are designed to not couple with EdenSCM.
For example, `testing.t`:
- Does not depend on anything in `edenscm`.
- Does not set test environment variables like `HGUSER`.
- Does not handle business-logic related syntaxes like `#chg-compatible`.

However, for EdenSCM tests there are needs for setting `HGUSER`, respecting
`#chg-compatible`, providing the right `hg` command, etc. This is done by
the `testing.ext.hg` extension.

An extension can have a `testsetup(t: TestTmp)` entry point to change the
`TestTmp`. It can register functions (ex. `hg`), set environment variables,
and do other setups allowed by Python.

An extension does not have to be under the `testing.ext` module. It can be any
importable Python module living in other places outside `edenscm`. This could
be useful for using the testing module to test other (CLI) tools.

"""
