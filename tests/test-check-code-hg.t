  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..
  $ if hg identify -q > /dev/null 2>&1; then :
  > else
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi

New errors are not allowed. Warnings are strongly discouraged.

  $ hg manifest 2>/dev/null \
  >   | xargs "$check_code" --warnings --nolineno --per-file=0 \
  >   || false

Check Python files without py extension

  $ cp \
  >   hg \
  >   hgweb.cgi \
  >   contrib/convert-repo \
  >   contrib/dumprevlog \
  >   contrib/hgweb.fcgi \
  >   contrib/hgweb.wsgi \
  >   contrib/simplemerge \
  >   contrib/undumprevlog \
  >   "$TESTTMP"/
  $ for f in "$TESTTMP"/*; do cp "$f" "$f.py"; done
  $ "$check_code" --warnings --nolineno --per-file=0 "$TESTTMP"/*.py \
  >   || false
