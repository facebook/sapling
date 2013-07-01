  $ "$TESTDIR/hghave" pyflakes || exit 80
  $ cd "`dirname "$TESTDIR"`"

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)
  $ hg manifest 2>/dev/null | egrep "\.py$|^[^.]*$" | grep -v /random_seed$ \
  > | xargs pyflakes 2>/dev/null | "$TESTDIR/filterpyflakes.py"
  contrib/simplemerge:*: 'os' imported but unused (glob)
  contrib/win32/hgwebdir_wsgi.py:*: 'win32traceutil' imported but unused (glob)
  setup.py:*: 'sha' imported but unused (glob)
  setup.py:*: 'zlib' imported but unused (glob)
  setup.py:*: 'bz2' imported but unused (glob)
  setup.py:*: 'py2exe' imported but unused (glob)
  tests/hghave.py:*: 'hgext' imported but unused (glob)
  tests/hghave.py:*: '_lsprof' imported but unused (glob)
  tests/hghave.py:*: 'publish_cmdline' imported but unused (glob)
  tests/hghave.py:*: 'pygments' imported but unused (glob)
  tests/hghave.py:*: 'ssl' imported but unused (glob)
  contrib/casesmash.py:*: local variable 'inst' is assigned to but never used (glob)
  contrib/check-code.py:*: local variable 'po' is assigned to but never used (glob)
  contrib/hgfixes/fix_leftover_imports.py:*: local variable 'bare_names' is assigned to but never used (glob)
  contrib/perf.py:*: local variable 'm' is assigned to but never used (glob)
  contrib/perf.py:*: local variable 'c' is assigned to but never used (glob)
  doc/hgmanpage.py:*: local variable 'backref_text' is assigned to but never used (glob)
  tests/hghave.py:*: local variable 'err' is assigned to but never used (glob)
  tests/test-hgweb-auth.py:*: local variable 'e' is assigned to but never used (glob)
  contrib/win32/hgwebdir_wsgi.py:*: 'from isapi.install import *' used; unable to detect undefined names (glob)
  hgext/inotify/linux/__init__.py:*: 'from _inotify import *' used; unable to detect undefined names (glob)
  

