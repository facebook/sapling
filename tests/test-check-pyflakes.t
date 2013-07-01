  $ "$TESTDIR/hghave" pyflakes || exit 80
  $ cd "`dirname "$TESTDIR"`"

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)
  $ hg manifest 2>/dev/null | egrep "\.py$|^[^.]*$" | grep -v /random_seed$ \
  > | xargs pyflakes 2>/dev/null | "$TESTDIR/filterpyflakes.py"
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
  contrib/win32/hgwebdir_wsgi.py:*: 'from isapi.install import *' used; unable to detect undefined names (glob)
  hgext/inotify/linux/__init__.py:*: 'from _inotify import *' used; unable to detect undefined names (glob)
  

