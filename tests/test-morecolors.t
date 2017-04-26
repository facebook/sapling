  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > morecolors=$TESTDIR/../hgext3rd/morecolors.py
  > [ui]
  > color=always
  > [color]
  > mode=ansi
  > EOF

Traceback has color:

  $ cat > repocrash.py << EOF
  > from mercurial import error
  > def reposetup(ui, repo):
  >     raise error.Abort('.')
  > EOF

  $ hg init repo1
  $ cd repo1

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > repocrash=$TESTTMP/repocrash.py
  > EOF

  $ hg commit --traceback 2>&1 | egrep -v '^  '
  Traceback (most recent call last):
  \x1b[0;31;1m  File "$TESTTMP/repocrash.py", line 3, in reposetup\x1b[0m (esc)
  \x1b[0;31;1m    raise error.Abort('.')\x1b[0m (esc)
  \x1b[0;31;1mAbort: .\x1b[0m (esc)
  abort: .

