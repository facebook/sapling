#require git no-eden no-windows


  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ export HGIDENTITY=sl

Prepare git repos.

  $ git init -q -b main sub-repo
  $ cd sub-repo
  $ echo 1 > x
  $ git add x
  $ git commit --quiet -m init

  $ cd
  $ git init -q -b main super-repo
  $ cd super-repo
  $ git submodule --quiet add -b main ../sub-repo/.git
  $ git commit -qm 'add .gitmodules'

The submodule uses a relative url.

  $ cat .gitmodules
  [submodule "sub-repo"]
  	path = sub-repo
  	url = ../sub-repo/.git
  	branch = main

sl-cloned repo can checkout the submodule

  $ cd
  $ sl clone --git $TESTTMP/super-repo super-repo-sl
  From $TESTTMP/super-repo
   * [new ref]         4d33120ba7e406443fef0b3223a6a5d4f2e7e111 -> remote/main
  pulling submodule sub-repo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd super-repo-sl
  $ cat sub-repo/x
  1
