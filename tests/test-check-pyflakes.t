  $ "$TESTDIR/hghave" pyflakes || exit 80
  $ cd `dirname $TESTDIR`
  $ pyflakes mercurial hgext 2>&1 | $TESTDIR/filterpyflakes.py
  hgext/inotify/linux/__init__.py:*: 'from _inotify import *' used; unable to detect undefined names (glob)
  

