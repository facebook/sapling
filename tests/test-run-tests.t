This file tests the behavior of run-tests.py itself.

Avoid interference from actual test env:

  $ unset HGTEST_JOBS
  $ unset HGTEST_TIMEOUT
  $ unset HGTEST_PORT
  $ unset HGTEST_SHELL

Smoke test with install
============

  $ run-tests.py $HGTEST_RUN_TESTS_PURE -l
  
  # Ran 0 tests, 0 skipped, 0 warned, 0 failed.

Define a helper to avoid the install step
=============
  $ rt()
  > {
  >     run-tests.py --with-hg=`which hg` "$@"
  > }

error paths

#if symlink
  $ ln -s `which true` hg
  $ run-tests.py --with-hg=./hg
  warning: --with-hg should specify an hg script
  
  # Ran 0 tests, 0 skipped, 0 warned, 0 failed.
  $ rm hg
#endif

#if execbit
  $ touch hg
  $ run-tests.py --with-hg=./hg
  Usage: run-tests.py [options] [tests]
  
  run-tests.py: error: --with-hg must specify an executable hg script
  [2]
  $ rm hg
#endif

an empty test
=======================

  $ touch test-empty.t
  $ rt
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  $ rm test-empty.t

a succesful test
=======================

  $ cat > test-success.t << EOF
  >   $ echo babar
  >   babar
  >   $ echo xyzzy
  >   never*happens (glob) (?)
  >   xyzzy
  >   nor this (?)
  >   $ printf 'abc\ndef\nxyz\n'
  >   123 (?)
  >   abc
  >   def (?)
  >   456 (?)
  >   xyz
  > EOF

  $ rt
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

failing test
==================

test churn with globs
  $ cat > test-failure.t <<EOF
  >   $ echo "bar-baz"; echo "bar-bad"
  >   bar*bad (glob)
  >   bar*baz (glob)
  > EOF
  $ rt test-failure.t
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,3 +1,3 @@
     $ echo "bar-baz"; echo "bar-bad"
  +  bar*baz (glob)
     bar*bad (glob)
  -  bar*baz (glob)
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 1 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

basic failing test
  $ cat > test-failure.t << EOF
  >   $ echo babar
  >   rataxes
  > This is a noop statement so that
  > this test is still more bytes than success.
  > pad pad pad pad............................................................
  > pad pad pad pad............................................................
  > EOF

  >>> fh = open('test-failure-unicode.t', 'wb')
  >>> fh.write(u'  $ echo babar\u03b1\n'.encode('utf-8')) and None
  >>> fh.write(u'  l\u03b5\u03b5t\n'.encode('utf-8')) and None

  $ rt
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  ERROR: test-failure.t output changed
  !.
  --- $TESTTMP/test-failure-unicode.t
  +++ $TESTTMP/test-failure-unicode.t.err
  @@ -1,2 +1,2 @@
     $ echo babar\xce\xb1 (esc)
  -  l\xce\xb5\xce\xb5t (esc)
  +  babar\xce\xb1 (esc)
  
  ERROR: test-failure-unicode.t output changed
  !
  Failed test-failure.t: output changed
  Failed test-failure-unicode.t: output changed
  # Ran 3 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]

test --xunit support
  $ rt --xunit=xunit.xml
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  ERROR: test-failure.t output changed
  !.
  --- $TESTTMP/test-failure-unicode.t
  +++ $TESTTMP/test-failure-unicode.t.err
  @@ -1,2 +1,2 @@
     $ echo babar\xce\xb1 (esc)
  -  l\xce\xb5\xce\xb5t (esc)
  +  babar\xce\xb1 (esc)
  
  ERROR: test-failure-unicode.t output changed
  !
  Failed test-failure.t: output changed
  Failed test-failure-unicode.t: output changed
  # Ran 3 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]
  $ cat xunit.xml
  <?xml version="1.0" encoding="utf-8"?>
  <testsuite errors="0" failures="2" name="run-tests" skipped="0" tests="3">
    <testcase name="test-success.t" time="*"/> (glob)
    <testcase name="test-failure-unicode.t" time="*"> (glob)
  <![CDATA[--- $TESTTMP/test-failure-unicode.t
  +++ $TESTTMP/test-failure-unicode.t.err
  @@ -1,2 +1,2 @@
     $ echo babar\xce\xb1 (esc)
  -  l\xce\xb5\xce\xb5t (esc)
  +  babar\xce\xb1 (esc)
  ]]>  </testcase>
    <testcase name="test-failure.t" time="*"> (glob)
  <![CDATA[--- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  ]]>  </testcase>
  </testsuite>

  $ rm test-failure-unicode.t

test for --retest
====================

  $ rt --retest
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Selecting Tests To Run
======================

successful

  $ rt test-success.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

success w/ keyword
  $ rt -k xyzzy
  .
  # Ran 2 tests, 1 skipped, 0 warned, 0 failed.

failed

  $ rt test-failure.t
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 1 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

failure w/ keyword
  $ rt -k rataxes
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  ERROR: test-failure.t output changed
  !
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Verify that when a process fails to start we show a useful message
==================================================================

  $ cat > test-serve-fail.t <<EOF
  >   $ echo 'abort: child process failed to start blah'
  > EOF
  $ rt test-serve-fail.t
  
  ERROR: test-serve-fail.t output changed
  !
  Failed test-serve-fail.t: server failed to start (HGPORT=*) (glob)
  # Ran 1 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]
  $ rm test-serve-fail.t

Verify that we can try other ports
===================================
  $ hg init inuse
  $ hg serve -R inuse -p $HGPORT -d --pid-file=blocks.pid
  $ cat blocks.pid >> $DAEMON_PIDS
  $ cat > test-serve-inuse.t <<EOF
  >   $ hg serve -R `pwd`/inuse -p \$HGPORT -d --pid-file=hg.pid
  >   $ cat hg.pid >> \$DAEMON_PIDS
  > EOF
  $ rt test-serve-inuse.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  $ rm test-serve-inuse.t

Running In Debug Mode
======================

  $ rt --debug 2>&1 | grep -v pwd
  + echo *SALT* 0 0 (glob)
  *SALT* 0 0 (glob)
  + echo babar
  babar
  + echo *SALT* 6 0 (glob)
  *SALT* 6 0 (glob)
  *+ echo *SALT* 0 0 (glob)
  *SALT* 0 0 (glob)
  + echo babar
  babar
  + echo *SALT* 2 0 (glob)
  *SALT* 2 0 (glob)
  + echo xyzzy
  xyzzy
  + echo *SALT* 6 0 (glob)
  *SALT* 6 0 (glob)
  + printf *abc\ndef\nxyz\n* (glob)
  abc
  def
  xyz
  + echo *SALT* 12 0 (glob)
  *SALT* 12 0 (glob)
  .
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.

Parallel runs
==============

(duplicate the failing test to get predictable output)
  $ cp test-failure.t test-failure-copy.t

  $ rt --jobs 2 test-failure*.t -n
  !!
  Failed test-failure*.t: output changed (glob)
  Failed test-failure*.t: output changed (glob)
  # Ran 2 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]

failures in parallel with --first should only print one failure
  >>> f = open('test-nothing.t', 'w')
  >>> f.write('foo\n' * 1024) and None
  >>> f.write('  $ sleep 1') and None
  $ rt --jobs 2 --first
  
  --- $TESTTMP/test-failure*.t (glob)
  +++ $TESTTMP/test-failure*.t.err (glob)
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  Failed test-failure*.t: output changed (glob)
  Failed test-nothing.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 2 failed.
  python hash seed: * (glob)
  [1]


(delete the duplicated test file)
  $ rm test-failure-copy.t test-nothing.t


Interactive run
===============

(backup the failing test)
  $ cp test-failure.t backup

Refuse the fix

  $ echo 'n' | rt -i
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  Accept this change? [n] 
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ cat test-failure.t
    $ echo babar
    rataxes
  This is a noop statement so that
  this test is still more bytes than success.
  pad pad pad pad............................................................
  pad pad pad pad............................................................

Interactive with custom view

  $ echo 'n' | rt -i --view echo
  $TESTTMP/test-failure.t $TESTTMP/test-failure.t.err (glob)
  Accept this change? [n]* (glob)
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

View the fix

  $ echo 'y' | rt --view echo
  $TESTTMP/test-failure.t $TESTTMP/test-failure.t.err (glob)
  
  ERROR: test-failure.t output changed
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

Accept the fix

  $ echo "  $ echo 'saved backup bundle to \$TESTTMP/foo.hg'" >> test-failure.t
  $ echo "  saved backup bundle to \$TESTTMP/foo.hg" >> test-failure.t
  $ echo "  $ echo 'saved backup bundle to \$TESTTMP/foo.hg'" >> test-failure.t
  $ echo "  saved backup bundle to \$TESTTMP/foo.hg (glob)" >> test-failure.t
  $ echo "  $ echo 'saved backup bundle to \$TESTTMP/foo.hg'" >> test-failure.t
  $ echo "  saved backup bundle to \$TESTTMP/*.hg (glob)" >> test-failure.t
  $ echo 'y' | rt -i 2>&1
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,11 +1,11 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
   pad pad pad pad............................................................
     $ echo 'saved backup bundle to $TESTTMP/foo.hg'
  -  saved backup bundle to $TESTTMP/foo.hg
  +  saved backup bundle to $TESTTMP/foo.hg* (glob)
     $ echo 'saved backup bundle to $TESTTMP/foo.hg'
     saved backup bundle to $TESTTMP/foo.hg* (glob)
     $ echo 'saved backup bundle to $TESTTMP/foo.hg'
  Accept this change? [n] ..
  # Ran 2 tests, 0 skipped, 0 warned, 0 failed.

  $ sed -e 's,(glob)$,&<,g' test-failure.t
    $ echo babar
    babar
  This is a noop statement so that
  this test is still more bytes than success.
  pad pad pad pad............................................................
  pad pad pad pad............................................................
    $ echo 'saved backup bundle to $TESTTMP/foo.hg'
    saved backup bundle to $TESTTMP/foo.hg (glob)<
    $ echo 'saved backup bundle to $TESTTMP/foo.hg'
    saved backup bundle to $TESTTMP/foo.hg (glob)<
    $ echo 'saved backup bundle to $TESTTMP/foo.hg'
    saved backup bundle to $TESTTMP/*.hg (glob)<

(reinstall)
  $ mv backup test-failure.t

No Diff
===============

  $ rt --nodiff
  !.
  Failed test-failure.t: output changed
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

test --tmpdir support
  $ rt --tmpdir=$TESTTMP/keep test-success.t
  
  Keeping testtmp dir: $TESTTMP/keep/child1/test-success.t (glob)
  Keeping threadtmp dir: $TESTTMP/keep/child1  (glob)
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

timeouts
========
  $ cat > test-timeout.t <<EOF
  >   $ sleep 2
  >   $ echo pass
  >   pass
  > EOF
  > echo '#require slow' > test-slow-timeout.t
  > cat test-timeout.t >> test-slow-timeout.t
  $ rt --timeout=1 --slowtimeout=3 test-timeout.t test-slow-timeout.t
  st
  Skipped test-slow-timeout.t: missing feature: allow slow tests
  Failed test-timeout.t: timed out
  # Ran 1 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]
  $ rt --timeout=1 --slowtimeout=3 \
  > test-timeout.t test-slow-timeout.t --allow-slow-tests
  .t
  Failed test-timeout.t: timed out
  # Ran 2 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]
  $ rm test-timeout.t test-slow-timeout.t

test for --time
==================

  $ rt test-success.t --time
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  # Producing time report
  start   end     cuser   csys    real      Test
  \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   test-success.t (re)

test for --time with --job enabled
====================================

  $ rt test-success.t --time --jobs 2
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.
  # Producing time report
  start   end     cuser   csys    real      Test
  \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   \s*[\d\.]{5}   test-success.t (re)

Skips
================
  $ cat > test-skip.t <<EOF
  >   $ echo xyzzy
  > #require false
  > EOF
  $ rt --nodiff
  !.s
  Skipped test-skip.t: missing feature: nail clipper
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ rt --keyword xyzzy
  .s
  Skipped test-skip.t: missing feature: nail clipper
  # Ran 2 tests, 2 skipped, 0 warned, 0 failed.

Skips with xml
  $ rt --keyword xyzzy \
  >  --xunit=xunit.xml
  .s
  Skipped test-skip.t: missing feature: nail clipper
  # Ran 2 tests, 2 skipped, 0 warned, 0 failed.
  $ cat xunit.xml
  <?xml version="1.0" encoding="utf-8"?>
  <testsuite errors="0" failures="0" name="run-tests" skipped="2" tests="2">
    <testcase name="test-success.t" time="*"/> (glob)
  </testsuite>

Missing skips or blacklisted skips don't count as executed:
  $ echo test-failure.t > blacklist
  $ rt --blacklist=blacklist --json\
  >   test-failure.t test-bogus.t
  ss
  Skipped test-bogus.t: Doesn't exist
  Skipped test-failure.t: blacklisted
  # Ran 0 tests, 2 skipped, 0 warned, 0 failed.
  $ cat report.json
  testreport ={
      "test-bogus.t": {
          "result": "skip"
      }, 
      "test-failure.t": {
          "result": "skip"
      }
  } (no-eol)
test for --json
==================

  $ rt --json
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  
  ERROR: test-failure.t output changed
  !.s
  Skipped test-skip.t: missing feature: nail clipper
  Failed test-failure.t: output changed
  # Ran 2 tests, 1 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]

  $ cat report.json
  testreport ={
      "test-failure.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "diff": "---.+\+\+\+.+", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "failure", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-skip.t": {
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "diff": "", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "skip", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-success.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "diff": "", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "success", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }
  } (no-eol)

Test that failed test accepted through interactive are properly reported:

  $ cp test-failure.t backup
  $ echo y | rt --json -i
  
  --- $TESTTMP/test-failure.t
  +++ $TESTTMP/test-failure.t.err
  @@ -1,5 +1,5 @@
     $ echo babar
  -  rataxes
  +  babar
   This is a noop statement so that
   this test is still more bytes than success.
   pad pad pad pad............................................................
  Accept this change? [n] ..s
  Skipped test-skip.t: missing feature: nail clipper
  # Ran 2 tests, 1 skipped, 0 warned, 0 failed.

  $ cat report.json
  testreport ={
      "test-failure.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "diff": "", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "success", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-skip.t": {
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "diff": "", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "skip", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }, ? (re)
      "test-success.t": [\{] (re)
          "csys": "\s*[\d\.]{4,5}", ? (re)
          "cuser": "\s*[\d\.]{4,5}", ? (re)
          "diff": "", ? (re)
          "end": "\s*[\d\.]{4,5}", ? (re)
          "result": "success", ? (re)
          "start": "\s*[\d\.]{4,5}", ? (re)
          "time": "\s*[\d\.]{4,5}" (re)
      }
  } (no-eol)
  $ mv backup test-failure.t

backslash on end of line with glob matching is handled properly

  $ cat > test-glob-backslash.t << EOF
  >   $ echo 'foo bar \\'
  >   foo * \ (glob)
  > EOF

  $ rt test-glob-backslash.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

  $ rm -f test-glob-backslash.t

Test reusability for third party tools
======================================

  $ mkdir "$TESTTMP"/anothertests
  $ cd "$TESTTMP"/anothertests

test that `run-tests.py` can execute hghave, even if it runs not in
Mercurial source tree.

  $ cat > test-hghave.t <<EOF
  > #require true
  >   $ echo foo
  >   foo
  > EOF
  $ rt test-hghave.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

test that RUNTESTDIR refers the directory, in which `run-tests.py` now
running is placed.

  $ cat > test-runtestdir.t <<EOF
  > - $TESTDIR, in which test-run-tests.t is placed
  > - \$TESTDIR, in which test-runtestdir.t is placed (expanded at runtime)
  > - \$RUNTESTDIR, in which run-tests.py is placed (expanded at runtime)
  > 
  > #if windows
  >   $ test "\$TESTDIR" = "$TESTTMP\anothertests"
  > #else
  >   $ test "\$TESTDIR" = "$TESTTMP"/anothertests
  > #endif
  >   $ test "\$RUNTESTDIR" = "$TESTDIR"
  >   $ head -n 3 "\$RUNTESTDIR"/../contrib/check-code.py
  >   #!/usr/bin/env python
  >   #
  >   # check-code - a style and portability checker for Mercurial
  > EOF
  $ rt test-runtestdir.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

#if execbit

test that TESTDIR is referred in PATH

  $ cat > custom-command.sh <<EOF
  > #!/bin/sh
  > echo "hello world"
  > EOF
  $ chmod +x custom-command.sh
  $ cat > test-testdir-path.t <<EOF
  >   $ custom-command.sh
  >   hello world
  > EOF
  $ rt test-testdir-path.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

#endif

test support for --allow-slow-tests
  $ cat > test-very-slow-test.t <<EOF
  > #require slow
  >   $ echo pass
  >   pass
  > EOF
  $ rt test-very-slow-test.t
  s
  Skipped test-very-slow-test.t: missing feature: allow slow tests
  # Ran 0 tests, 1 skipped, 0 warned, 0 failed.
  $ rt $HGTEST_RUN_TESTS_PURE --allow-slow-tests test-very-slow-test.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

support for running a test outside the current directory
  $ mkdir nonlocal
  $ cat > nonlocal/test-is-not-here.t << EOF
  >   $ echo pass
  >   pass
  > EOF
  $ rt nonlocal/test-is-not-here.t
  .
  # Ran 1 tests, 0 skipped, 0 warned, 0 failed.

support for bisecting failed tests automatically
  $ hg init bisect
  $ cd bisect
  $ cat >> test-bisect.t <<EOF
  >   $ echo pass
  >   pass
  > EOF
  $ hg add test-bisect.t
  $ hg ci -m 'good'
  $ cat >> test-bisect.t <<EOF
  >   $ echo pass
  >   fail
  > EOF
  $ hg ci -m 'bad'
  $ rt --known-good-rev=0 test-bisect.t
  
  --- $TESTTMP/anothertests/bisect/test-bisect.t
  +++ $TESTTMP/anothertests/bisect/test-bisect.t.err
  @@ -1,4 +1,4 @@
     $ echo pass
     pass
     $ echo pass
  -  fail
  +  pass
  
  ERROR: test-bisect.t output changed
  !
  Failed test-bisect.t: output changed
  test-bisect.t broken by 72cbf122d116 (bad)
  # Ran 1 tests, 0 skipped, 0 warned, 1 failed.
  python hash seed: * (glob)
  [1]
