  $ "$TESTDIR/hghave" pyflakes || exit 80
  $ cd $(dirname $TESTDIR)
  $ pyflakes mercurial hgext 2>&1 | $TESTDIR/filterpyflakes.py
  mercurial/hgweb/server.py:*: 'activeCount' imported but unused (glob)
  mercurial/commands.py:*: 'base85' imported but unused (glob)
  mercurial/commands.py:*: 'bdiff' imported but unused (glob)
  mercurial/commands.py:*: 'mpatch' imported but unused (glob)
  mercurial/commands.py:*: 'osutil' imported but unused (glob)
  mercurial/revlog.py:*: 'short' imported but unused (glob)
  

