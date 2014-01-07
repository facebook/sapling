  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..
  $ if hg identify -q > /dev/null 2>&1; then :
  > else
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi

Prepare check for Python files without py extension

  $ cp \
  >   hg \
  >   hgweb.cgi \
  >   contrib/convert-repo \
  >   contrib/dumprevlog \
  >   contrib/hgweb.fcgi \
  >   contrib/hgweb.wsgi \
  >   contrib/simplemerge \
  >   contrib/undumprevlog \
  >   i18n/hggettext \
  >   i18n/posplit \
  >   tests/hghave \
  >   tests/dummyssh \
  >   "$TESTTMP"/
  $ for f in "$TESTTMP"/*; do mv "$f" "$f.py"; done

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ { hg manifest 2>/dev/null; ls "$TESTTMP"/*.py | sed 's-\\-/-g'; } |
  >   xargs "$check_code" --warnings --per-file=0 || false
  Skipping hgext/zeroconf/Zeroconf.py it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/socketutil.py it has no-che?k-code (glob)
