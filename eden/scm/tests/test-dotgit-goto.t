#require git

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ setconfig workingcopy.rust-checkout=true
  $ enable shelve morestatus
  $ setconfig morestatus.show=true

Prepare git repo

  $ git init -q -b main git-repo

  $ cd git-repo
  $ HGIDENTITY=sl drawdag --no-bookmarks << 'EOS'
  > A..C
  > EOS

Go forward

  $ sl go -q $A
  $ sl go -q $B

Status should be clean

  $ sl status

Go backward

  $ sl go -q $A

#if no-windows
tofix: make the test case work on windows
Test go conflicts
  $ cd ..
  $ git init -q -b main git-conflict-repo
  $ cd git-conflict-repo
  $ echo 1 > a
  $ sl ci -Aqm A
  $ echo 1b > a
  $ sl ci -m B
  $ sl go -q 'desc(A)'
  $ echo 1c > a
  $ sl go 'desc(B)'
  abort: Command exited with code 1
    git --git-dir=$TESTTMP/git-conflict-repo/.git checkout -d --recurse-submodules 8a951cf493d544cc7dc517a74774913b6dfc6015
      error: Your local changes to the following files would be overwritten by checkout:
      	a
      Please commit your changes or stash them before you switch branches.
      Aborting
  
  [255]
tofix: should have a way to get out of the unfinished *update* state
  $ sl st
  M a
  
  # The repository is in an unfinished *update* state.
  # To continue:                sl go 'desc(B)'
  # To abort:                   sl goto . --clean    (warning: this will discard uncommitted changes)
  $ sl shelve
  abort: interrupted goto
  (use 'sl goto --continue' to continue or
       'sl goto --clean' to abort - WARNING: will destroy uncommitted changes)
  [255]
  $ sl goto --continue
  abort: not in an interrupted update state
  [255]
  $ sl continue
  abort: nothing to continue
  [255]
  $ sl goto --clean 'desc(B)'
  update complete
  $ sl st
#endif
