#require py2
#chg-compatible

  $ disable treemanifest
  $ configure dummyssh
#require killdaemons

Test wire protocol argument passing

Setup repo:

  $ hg init repo

Local:

  $ hg debugwireargs repo eins zwei --three drei --four vier
  eins zwei drei vier None
  $ hg debugwireargs repo eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs repo eins zwei
  eins zwei None None None
  $ hg debugwireargs repo eins zwei --five fuenf
  eins zwei None None fuenf

SSH (try to exercise the ssh functionality with a dummy script):

  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo uno due tre quattro
  uno due tre quattro None
  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo eins zwei --four vier
  eins zwei None vier None
  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo eins zwei
  eins zwei None None None
  $ hg debugwireargs --ssh "\"$PYTHON\" $TESTDIR/dummyssh" ssh://user@dummy/repo eins zwei --five fuenf
  eins zwei None None None
