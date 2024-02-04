#require git no-windows
#debugruntest-compatible


  $ eagerepo
  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ enable rebase
  $ export HGIDENTITY=sl

Submodule that refers to itself in mod/doc/

  $ git init -q -b main git-sub
  $ cd git-sub

  $ echo A > A
  $ git add A
  $ git commit -q -m A

  $ git checkout -q -b doc
  $ echo B > B
  $ git add B
  $ git commit -q -m B

  $ git checkout -q main

  $ URL=file://$TESTTMP/git-sub/.git
  $ git submodule --quiet add -b doc $URL mod/doc

  $ git commit -qm C

Main repo that refers to the submodule repo using the same URL

  $ cd
  $ git init -q -b main git-main
  $ cd git-main

  $ git submodule --quiet add -b main $URL mod/sub
  $ git commit -qm A

Sapling should be able to clone the repo

  $ cd
  $ sl clone --git "file://$TESTTMP/git-main/" sl-main
  From file:/*/$TESTTMP/git-main (glob)
   * [new ref]  * (glob)
  pulling submodule mod/sub
  pulling submodule mod/sub/mod/doc
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd sl-main
  $ find .
  mod
  mod/sub
  mod/sub/A
  mod/sub/mod
  mod/sub/mod/doc
  mod/sub/mod/doc/A
  mod/sub/mod/doc/B
