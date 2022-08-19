#debugruntest-compatible
#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

#require symlink

#testcases legacyupdater rustupdater

  $ configure modern
#if rustupdater
  $ setconfig remotefilelog.useruststore=True
#endif

  $ newserver server1
  $ clone server1 client

  $ cd client
  $ echo 'CONTENT' > file
  $ ln -s file link
  $ hg commit -qAm "first commit"
  $ hg rm link
  $ echo 'NO LONGER A LINK' > link
  $ hg commit -qAm "second"
  $ hg prev
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [7e5b26] first commit
  $ hg next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [3165d5] second
  $ cat link
  NO LONGER A LINK
  $ cat file
  CONTENT

Make sure we can clear out unknown symlink to write regular file.
  $ mkdir subdir
  $ echo subfile > subdir/subfile
  $ hg commit -qAm subfile
  $ rm -rf subdir
  $ ln -s file subdir
Sanity that we can't do it without -C
  $ hg up -q .
  $ cat subdir/subfile
  cat: subdir/subfile: $ENOTDIR$
  [1]
  $ hg up -Cq .
  $ cat subdir/subfile
  subfile
