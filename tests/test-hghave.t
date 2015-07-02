Testing that hghave does not crash when checking features

  $ hghave --test-features 2>/dev/null

Testing hghave extensibility for third party tools

  $ cat > hghaveaddon.py <<EOF
  > import hghave
  > @hghave.check("custom", "custom hghave feature")
  > def has_custom():
  >     return True
  > EOF

(invocation via run-tests.py)

  $ cat > test-hghaveaddon.t <<EOF
  > #require custom
  >   $ echo foo
  >   foo
  > EOF
  $ run-tests.py test-hghaveaddon.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

(invocation via command line)

  $ unset TESTDIR
  $ hghave custom

(terminate with exit code 2 at failure of importing hghaveaddon.py)

  $ rm hghaveaddon.*
  $ cat > hghaveaddon.py <<EOF
  > importing this file should cause syntax error
  > EOF

  $ hghave custom
  failed to import hghaveaddon.py from '.': invalid syntax (hghaveaddon.py, line 1)
  [2]
