#require git

  $ eagerepo
  $ . $TESTDIR/git.sh
  $ enable smartlog amend

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
  $ sl clone --git -q file://$TESTTMP/upstream client
  $ cd client
  $ sl smartlog -T '{desc} {remotebookmarks}'
  @  bar remote/main
  │
  ~

Add "fork" as another remote:
  $ sl path --add fork file://$TESTTMP/fork
  $ sl smartlog -T '{desc} {remotebookmarks}'
  @  bar remote/main
  │
  ~

Can check out our branch without first pulling:
  $ sl goto -q fork/existing-branch
  $ sl smartlog -T '{desc} {remotebookmarks}'
  @  existing-branch fork/existing-branch
  │
  o  bar remote/main
  │
  ~

Prepare and push a new branch:
  $ sl up -q main
  $ touch baz
  $ sl commit -qAm baz
  $ touch qux
  $ sl commit -qAm qux
  $ sl push -q fork --to my-branch

Test that our stack still shows in smartlog after pushing:
  $ sl smartlog -T '{desc} {remotebookmarks}'
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
  $ sl up -q main
  $ sl hide -q 'desc("existing-branch")'
  $ sl smartlog -T '{desc} {remotebookmarks}'
  o  qux fork/my-branch
  │
  o  baz
  │
  @  bar remote/main
  │
  ~
