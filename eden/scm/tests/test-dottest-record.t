#require no-eden no-windows

Prepare a simple test:

  $ cat > test-record.t << 'EOF'
  >   $ mkdir -p files/
  >   $ echo Line 2 > files/a
  >   $ echo Line 3 > files/b
  >   $ export FOO=BAR
  >   $ cd files
  > EOF

Cannot restore test without a record:

  $ sl debugrestoretest test-record.t --line 1
  abort: no recording found for test
  (use 'sl .t --record' to record a test run)
  [255]

Record a test:

  $ sl .t --record test-record.t
  # Ran 1 tests, 0 skipped, 0 failed.

Restore test state:

In line 1, FOO=BAR is not set, and cwd is at test root:

  $ SCRIPT=$(sl debugrestoretest test-record.t --line 1)
  $ grep '^cd ' $SCRIPT
  cd $TESTTMP/test-record.* (glob)
  $ grep FOO= $SCRIPT
  [1]

In line 2, "Line 2" is written to files/a:

  $ SCRIPT=$(sl debugrestoretest test-record.t --line 2)
  $ find $(dirname $SCRIPT)/files
  $TESTTMP/test-record.*/files/a (glob)
  $ cat $(dirname $SCRIPT)/files/a
  Line 2

In line 3, "Line 3" is written to files/b:

  $ SCRIPT=$(sl debugrestoretest test-record.t --line 3)
  $ find $(dirname $SCRIPT)/files
  $TESTTMP/test-record.*/files/a (glob)
  $TESTTMP/test-record.*/files/b (glob)
  $ cat $(dirname $SCRIPT)/files/b
  Line 3

In line 5, FOO=BAR is set, and cwd is "files/":

  $ SCRIPT=$(sl debugrestoretest test-record.t --line 5)
  $ grep '^cd ' $SCRIPT
  cd $TESTTMP/test-record.*/files (glob)
  $ grep FOO= $SCRIPT
  export FOO=BAR
