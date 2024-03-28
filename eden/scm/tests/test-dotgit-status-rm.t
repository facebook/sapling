#require git no-windows
#debugruntest-compatible

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true

Prepare git repo

  $ git init -q -b main git-repo

  $ cd git-repo
  $ HGIDENTITY=sl drawdag --no-bookmarks << 'EOS'
  > A..C
  > EOS

  $ sl go -q $A

Commit file removal

  $ rm A
  $ sl status
  ! A

  $ sl rm A
  $ sl status
  R A

  $ sl commit -m "Remove A"

Status should be clean

  $ sl status

Git status should be clean too

  $ git status --porcelain
