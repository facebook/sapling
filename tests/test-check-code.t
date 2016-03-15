#require test-repo

  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ hg locate | sed 's-\\-/-g' |
  >   xargs "$check_code" --warnings --per-file=0 || false
  Skipping hgext/fsmonitor/pywatchman/__init__.py it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/bser.c it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/capabilities.py it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/msc_stdint.h it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/pybser.py it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/socketutil.py it has no-che?k-code (glob)
