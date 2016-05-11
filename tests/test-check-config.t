#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.

  $ hg files "set:(**.py or **.txt) - tests/**" | sed 's|\\|/|g' |
  >   python contrib/check-config.py
