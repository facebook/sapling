  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..
  $ if hg identify -q > /dev/null; then :
  > else
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi
  $ hg manifest | xargs "$check_code" || echo 'FAILURE IS NOT AN OPTION!!!'

  $ hg manifest | xargs "$check_code" --warnings --nolineno --per-file=0 || true
