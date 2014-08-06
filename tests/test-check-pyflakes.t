#require test-repo pyflakes

  $ cd "`dirname "$TESTDIR"`"

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)

  $ hg locate 'set:**.py or grep("^!#.*python")' 2>/dev/null \
  > | xargs pyflakes 2>/dev/null | "$TESTDIR/filterpyflakes.py"
  contrib/win32/hgwebdir_wsgi.py:*: 'win32traceutil' imported but unused (glob)
  setup.py:*: 'sha' imported but unused (glob)
  setup.py:*: 'zlib' imported but unused (glob)
  setup.py:*: 'bz2' imported but unused (glob)
  setup.py:*: 'py2exe' imported but unused (glob)
  tests/hghave.py:*: '_lsprof' imported but unused (glob)
  tests/hghave.py:*: 'publish_cmdline' imported but unused (glob)
  tests/hghave.py:*: 'pygments' imported but unused (glob)
  tests/hghave.py:*: 'ssl' imported but unused (glob)
  contrib/win32/hgwebdir_wsgi.py:93: 'from isapi.install import *' used; unable to detect undefined names (glob)
  tests/filterpyflakes.py:58: undefined name 'undefinedname'
  

