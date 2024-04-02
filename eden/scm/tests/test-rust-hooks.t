#debugruntest-compatible

#require no-eden


Note that Rust hook execution escapes the debugruntest execution environment.
For now that is desirable since I also want to test the platform specific Rust spawn logic.

  $ configure modernclient

  $ hg debugtestcommand

Make sure alias marked "legacy:" works.
  $ hg debugoldtestcommand

Test hooks outside of repo:

#if windows
  $ setconfig 'hooks.pre-debugtestcommand.args=echo PRE_ARGS: %HG_ARGS%'
  $ setconfig 'hooks.pre-debugtestcommand.pwd=cd'
  $ setconfig 'hooks.pre-debugtestcommand.background=background:echo background & type nul > touched'
  $ setconfig 'hooks.fail-debugtestcommand=echo FAIL'
  $ hg debugtestcommand "hello ' there" foo
  PRE_ARGS: debugtestcommand "hello ' there" foo\r (esc)
  $TESTTMP\r (esc)
#else
  $ setconfig 'hooks.pre-debugtestcommand=echo PRE_ARGS: $HG_ARGS && echo HG: $HG && echo PWD: $PWD'
  $ setconfig 'hooks.pre-debugtestcommand.background=background:echo background > touched'
  $ setconfig 'hooks.post-debugtestcommand=echo POST_ARGS: $HG_ARGS && echo HG: $HG && echo RESULT: $HG_RESULT && echo PWD: $PWD'
  $ setconfig 'hooks.fail-debugtestcommand=echo FAIL'
  $ hg debugtestcommand "hello ' there" foo
  PRE_ARGS: debugtestcommand 'hello '\'' there' foo
  HG: *hg* (glob)
  PWD: $TESTTMP
  POST_ARGS: debugtestcommand 'hello '\'' there' foo
  HG: *hg* (glob)
  RESULT: 0
  PWD: $TESTTMP
#endif

Wait for background hook to touch file.
  $ sleep 1
  $ ls touched
  touched

  $ newclientrepo
  $ mkdir subdir
  $ cd subdir

When in repo, hooks run from repo root
#if windows
  $ hg debugtestcommand
  PRE_ARGS: debugtestcommand\r (esc)
  $TESTTMP\repo1\r (esc)
#else
  $ hg debugtestcommand
  PRE_ARGS: debugtestcommand
  HG: *hg* (glob)
  PWD: $TESTTMP/repo1
  POST_ARGS: debugtestcommand
  HG: *hg* (glob)
  RESULT: 0
  PWD: $TESTTMP/repo1
#endif

  $ cd ..

Wait for background hook to touch file.
  $ sleep 1
  $ hg st
  ? touched

Reset our hook config:
  $ rm $HGRCPATH
  $ configure modernclient

Test fail hooks:
  $ setconfig 'hooks.pre-debugtestcommand=echo PRE'
  $ setconfig 'hooks.post-debugtestcommand=echo POST'
  $ setconfig 'hooks.fail-debugtestcommand=echo FAIL'

#if windows
  $ hg debugtestcommand --abort
  PRE\r (esc)
  FAIL\r (esc)
  abort: aborting
  [255]
#else
  $ hg debugtestcommand --abort
  PRE
  FAIL
  abort: aborting
  [255]
#endif

Test hooks work for legacy command name:
  $ setconfig 'hooks.pre-debugoldtestcommand=echo LEGACY'
Both fire for new command name:
#if windows
  $ hg debugtestcommand
  PRE\r (esc)
  LEGACY\r (esc)
  POST\r (esc)
#else
  $ hg debugtestcommand
  PRE
  LEGACY
  POST
#endif

Both fire for old command name:
#if windows
  $ hg debugoldtestcommand
  PRE\r (esc)
  LEGACY\r (esc)
  POST\r (esc)
#else
  $ hg debugoldtestcommand
  PRE
  LEGACY
  POST
#endif

Warn about python hooks since we can't fall back to Python:
  $ newclientrepo
  $ setconfig 'hooks.pre-debugtestcommand.python=python:foo.py'
  $ hg debugtestcommand
  WARNING: not running python hooks ["pre-debugtestcommand.python"]


"pre" hooks abort on error:
  $ newclientrepo
  $ setconfig hooks.pre-debugtestcommand=oopsie-daisy hooks.post-debugtestcommand=oopsie-daisy hooks.fail-debugtestcommand=oopsie-daisy
#if windows
  $ hg debugtestcommand --echo running
  'oopsie-daisy' is not recognized as an internal or external command,\r (esc)
  operable program or batch file.\r (esc)
  abort: pre-debugtestcommand hook exited with status 1
  [255]
#else
  $ hg debugtestcommand --echo running
  * command not found (glob)
  abort: pre-debugtestcommand hook exited with status 127
  [255]
#endif

#if no-windows
Test client correlator:
  $ newclientrepo
  $ hg debugtestcommand --config 'hooks.pre-debugtestcommand=echo ENTRY POINT: $SAPLING_CLIENT_ENTRY_POINT && echo CORRELATOR: $SAPLING_CLIENT_CORRELATOR'
  ENTRY POINT: sapling
  CORRELATOR: test-correlator
#endif
