#require git no-eden

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true experimental.git-index-fast-path=true

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
