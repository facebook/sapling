#require no-eden no-windows git

  $ . $TESTDIR/git.sh
  $ . "$TESTDIR/histedit-helpers.sh"
  $ enable histedit

setup repo

  $ hg init --git repo1
  $ cd repo1
  $ touch A1 && hg commit -Am "A1" -d '1 0' -q
  $ touch A2 && hg commit -Am "A2" -d '2 0' -q
  $ touch A3 && hg commit -Am "A3" -d '3 0' -q
  $ touch A4 && hg commit -Am "A4" -d '4 0' -q

folding should work

  $ hg histedit --commands - << EOF
  > pick fd2a67d81220 'A1'
  > pick bc8bd49c677f 'A2'
  > fold 081c9e396fa1 'A3'
  > pick 91ad706dafee 'A4'
  > EOF
  $ hg st
  $ tglog
  @  17e3aa7dc15f 'A4'
  │
  o  594d4e89c0ff 'A2
  │
  │  ***
  │  A3'
  o  fd2a67d81220 'A1'
