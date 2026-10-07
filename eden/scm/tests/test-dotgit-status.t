#require git no-eden

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ enable absorb

Prepare git repo

  $ git init -q -b main git-repo
  $ cd git-repo
  $ echo 'i' > .gitignore 
  $ touch a b c
  $ git add a b c .gitignore
  $ git commit -q -m commit1
  $ for i in a b c; do echo 1 >> $i; done
  $ git commit -q -a -m commit2

Ignore status

  $ touch i

  $ git status --porcelain
  $ git status --porcelain --ignored
  !! i
  $ sl status
  $ sl status --ignore
  I i

Status when run from a sub-directory:

  $ mkdir foo
  $ cd foo
  $ sl status
  $ cd ..

Status after changing filesystem (modify, create, remove)

  $ echo 2 > b
  $ echo 2 > d
  $ rm c

  $ git status --porcelain
   M b
   D c
  ?? d

  $ sl status
  M b
  ! c
  ? d

Status update via add or remove commands

  $ sl rm c
  $ sl add d
  $ sl status
  M b
  A d
  R c

Clean up (revert, purge)

  $ sl revert --all -q --no-backup
  $ sl purge --files
  $ sl status
  $ git status --porcelain

`debugstatus` does not crash

  $ sl debugstatus
  len(dirstate) = not supproted
  len(nonnormal) = 0
  len(filtered nonnormal) = 0
  clock = None

Changed in the staging area, but not changed in the working copy

  $ echo 3 >> b
  $ git add b
  $ sl revert b --no-backup
  $ sl status
  $ sl diff
  $ git status --porcelain
  MM b

Clean stage after commiting modified, added, and removed files

  $ echo 3 >> a
  $ echo 3 > d
  $ rm b
  $ sl addremove --quiet
  $ sl status
  M a
  A d
  R b
  $ sl commit -m "commit3" 
  $ git ls-files --debug c | grep "mtime: 0:0"
  [1]
  >>> assert "mtime: 0:0" not in _, "cache entry of unchanged file c should not have been invalidated"
  $ sl status
  $ git status --porcelain

Handle Tree Changes

  $ mkdir -p some/dir
  $ touch some/dir/file1 some/dir/file2 some/dir/file3
  $ sl add some --quiet 
  $ sl commit -m "add some/dir/*"
  $ sl status
  $ git status --porcelain

  $ echo 1 >> some/dir/file1
  $ sl commit -m "update some/dir/file1"
  $ sl status
  $ git status --porcelain

  $ rm -rf some
  $ echo 1 > some
  $ sl addremove --quiet
  $ sl commit -m "replace dir with file of the same name"
  $ sl status
  $ git status --porcelain

Clean stage after amend

  $ echo 2 >> some
  $ sl amend
  $ sl status
# FIXME: the git index still holds the pre-amend tree; it should be clean.
  $ git status --porcelain
  MM some

  $ echo 3 >> some
  $ sl --config experimental.git-index-fast-path=false amend
  $ sl status
# FIXME: same with the index fast path disabled.
  $ git status --porcelain
  MM some

Clean stage after uncommit

  $ echo 1 > e
  $ sl commit -Aqm "add e"
  $ sl uncommit
  $ sl revert --all -q --no-backup
  $ sl status
  ? e
# FIXME: the git index still holds the uncommitted tree; it should say "?? e".
  $ git status --porcelain
  A  e
  $ rm e

Clean stage after absorb

  $ printf '1\n2\n' > f
  $ sl commit -Aqm "add f"
  $ echo 1 > g
  $ sl commit -Aqm "add g"
  $ printf '1\n2 edited\n' > f
  $ sl absorb -qa
  $ sl status
# FIXME: the git index still holds the pre-absorb tree; it should be clean.
  $ git status --porcelain
  MM f
