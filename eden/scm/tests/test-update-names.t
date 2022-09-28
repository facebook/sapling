#debugruntest-compatible
#chg-compatible

Test update logic when there are renames or weird same-name cases between dirs
and files

  $ configure modernclient
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig experimental.nativecheckout=true

Update with local changes across a file rename

  $ newclientrepo r1

  $ echo a > a
  $ hg add a
  $ hg ci -m a

  $ hg mv a b
  $ hg ci -m rename

  $ echo b > b
  $ hg ci -m change

  $ hg up -q cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b

  $ echo c > a

  $ hg up
  merging a and b to b
  warning: 1 conflicts while merging b! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]

Test update when local untracked directory exists with the same name as a
tracked file in a commit we are updating to
  $ newclientrepo r2
  $ echo root > root && hg ci -Am root  # rev 0
  adding root
  $ echo text > name && hg ci -Am "name is a file"  # rev 1
  adding name
  $ hg up 'desc(root)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir name
  $ hg up 'desc(name)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test update when local untracked directory exists with some files in it and has
the same name a tracked file in a commit we are updating to. In future this
should be updated to give an friendlier error message, but now we should just
make sure that this does not erase untracked data
  $ hg up 'desc(root)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir name
  $ echo text > name/file
  $ hg st
  ? name/file
  $ hg up 'desc(name)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

#if symlink

Test update when two commits have symlinks that point to different folders
  $ newclientrepo r3
  $ echo root > root && hg ci -Am root
  adding root
  $ mkdir folder1 && mkdir folder2
  $ ln -s folder1 folder
  $ hg ci -Am "symlink to folder1"
  adding folder
  $ rm folder
  $ ln -s folder2 folder
  $ hg ci -Am "symlink to folder2"
  $ hg up baf33104e0bc1162a796504d4eecd1159f3a6e93
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

#endif

#if rmcwd

Test that warning is printed if cwd is deleted during update
  $ newclientrepo r4
  $ mkdir dir
  $ cd dir
  $ echo a > a
  $ echo b > b
  $ hg add a b
  $ hg ci -m "file and dir"
  $ hg up -q null
  current directory was removed
  (consider changing to repo root: $TESTTMP/r4)

#endif
