  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > morecolors=
  > [ui]
  > color=always
  > [color]
  > mode=ansi
  > EOF

Traceback has color:

  $ cat > repocrash.py << EOF
  > from edenscm.mercurial import error
  > def reposetup(ui, repo):
  >     raise error.Abort('.')
  > EOF

  $ hg init repo1
  $ cd repo1

  $ hg commit --config extensions.repocrash=$TESTTMP/repocrash.py --traceback 2>&1 | egrep -v '^  '
  Traceback (most recent call last):
  \x1b[0;31;1m  File "$TESTTMP/repocrash.py", line 3, in reposetup\x1b[0m (esc)
  \x1b[0;31;1m    raise error.Abort('.')\x1b[0m (esc)
  \x1b[0;31;1mAbort: .\x1b[0m (esc)
  \x1b[0;91mabort:\x1b[0m . (esc)

Uncaught exception has color:

  $ cat > $TESTTMP/uncaughtcrash.py <<EOF
  > def reposetup(ui, repo):
  >     raise RuntimeError('.')
  > EOF

  $ hg commit --config extensions.repocrash=$TESTTMP/uncaughtcrash.py 2>&1 | egrep -v '^  '
  Traceback (most recent call last):
  \x1b[0;31;1m  File "$TESTTMP/uncaughtcrash.py", line 2, in reposetup\x1b[0m (esc)
  \x1b[0;31;1m    raise RuntimeError('.')\x1b[0m (esc)
  \x1b[0;31;1mRuntimeError: .\x1b[0m (esc)
