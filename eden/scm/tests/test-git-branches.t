#chg-compatible
#require git no-windows

  $ . $TESTDIR/git.sh
  $ enable smartlog amend
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

Prepare forked server repo with branch "existing-branch" off main:

  $ cd
  $ git clone -q upstream fork
  $ cd fork
  $ git checkout -qb existing-branch
  $ echo fork-existing-branch > existing-branch
  $ git add existing-branch
  $ git commit -qa -m existing-branch


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

Can check out our branch without first pulling:
  $ hg goto -q fork/existing-branch
  $ hg smartlog -T '{desc} {remotebookmarks}'
  @  existing-branch fork/existing-branch
  │
  o  bar remote/main
  │
  ~

Prepare and push a new branch:
  $ hg up -q main
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
  │ o  existing-branch fork/existing-branch
  ├─╯
  o  bar remote/main
  │
  ~

Make sure we can hide branches:
  $ hg up -q main
  $ hg hide -q 'desc("existing-branch")'
  $ hg smartlog -T '{desc} {remotebookmarks}'
  o  qux fork/my-branch
  │
  o  baz
  │
  @  bar remote/main
  │
  ~
