
  $ check_code="$TESTDIR"/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ EXTRAS=`python -c 'import lz4revlog' 2> /dev/null && echo "--config extensions.lz4revlog="`
  $ hg $EXTRAS locate | sed 's-\\-/-g' |
  >   xargs "$check_code" --warnings --per-file=0 || false
