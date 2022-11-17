#chg-compatible
#require git no-windows

  $ . $TESTDIR/git.sh
  $ enable smartlog
  $ setconfig workingcopy.ruststatus=False

Prepare upstream server repo w/ two commits on "main":

  $ git init -q upstream
  $ cd upstream
  $ git branch -m main
  $ echo foo > foo
  $ git add foo
  $ git commit -qa -m foo
  $ echo bar > bar
  $ git add bar
  $ git commit -qa -m bar

Prepare forked server repo:

  $ cd
  $ git clone -q upstream fork


Clone the upstream repo:
  $ cd
  $ hg clone --git -q file://$TESTTMP/upstream client
  $ cd client
  $ hg smartlog -T '{desc} {remotebookmarks}'
  @  bar remote/main
  │
  ~

Add "fork" as another remote:
  $ hg path --add fork file://$TESTTMP/fork
  $ hg smartlog -T '{desc} {remotebookmarks}'
  @  bar remote/main
  │
  ~

  $ touch baz
  $ hg commit -qAm baz
  $ touch qux
  $ hg commit -qAm qux
  $ hg push -q fork --to my-branch

Test that our stack still shows in smartlog after pushing:
  $ hg smartlog -T '{desc} {remotebookmarks}'
  @  qux fork/my-branch
  │
  o  baz
  │
  o  bar remote/main
  │
  ~
