#chg-compatible

#require symlink

#testcases legacyupdater rustupdater

  $ configure modern
#if rustupdater
  $ setconfig remotefilelog.useruststore=True
  $ setconfig worker.rustworkers=True
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
