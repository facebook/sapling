#require git no-windows
#debugruntest-compatible

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true

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
BUG: status is not clean

  $ sl status
  M A
  M B

Go backward
BUG: cannot go back

  $ sl go -q $A
  abort: 1 conflicting file changes:
   B
  (commit, shelve, goto --clean to discard all your changes, or update --merge to merge them)
  [255]
