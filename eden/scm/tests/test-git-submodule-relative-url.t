#require git no-windows no-eden


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
FIXME: does not work yet

  $ cd
  $ sl clone --git $TESTTMP/super-repo super-repo-sl
  From $TESTTMP/super-repo
   * [new ref]         4d33120ba7e406443fef0b3223a6a5d4f2e7e111 -> remote/main
  pulling submodule sub-repo
  fatal: '../sub-repo/.git' does not appear to be a git repository
  fatal: Could not read from remote repository.
  
  Please make sure you have the correct access rights
  and the repository exists.
  pulling submodule sub-repo
  fatal: '../sub-repo/.git' does not appear to be a git repository
  fatal: Could not read from remote repository.
  
  Please make sure you have the correct access rights
  and the repository exists.
  abort: unknown revision 'dbe0efed3c0f1e46be76159771bf3d15b8049a60'!
  [255]
  $ cd super-repo-sl
  $ cat sub-repo/x
  cat: sub-repo/x: $ENOENT$
  [1]
