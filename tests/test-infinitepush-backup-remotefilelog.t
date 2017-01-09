
  $ . $TESTDIR/require-ext.sh remotefilelog
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup infinitepush and remotefilelog server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cat >> .hg/hgrc << EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ cd ..

Make client shallow clone
  $ hgcloneshallow ssh://user@dummy/repo client
  streaming all changes
  0 files to transfer, 0 bytes of data
  transferred 0 bytes in 0.0 seconds (0 bytes/sec)
  streaming all changes
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Create 3 commits, two of which will be stripped. It's important to remove file
that was created in the second commit to make sure it's filelogs won't be
downloaded to the client
  $ cd repo
  $ mkcommit serverinitialcommit
  $ mkcommit committostripfirst
  $ hg rm committostripfirst
  $ echo 'committostripsecond' >> committostripsecond
  $ hg add committostripsecond
  $ hg ci -m committostripsecond

Pull changes client-side
  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/repo
  streaming all changes
  5 files to transfer, 1.06 KB of data
  transferred 1.06 KB in [\d.]+ seconds \([\d.]+ KB/sec\) (re)
  searching for changes
  no changes found

Make commit on top of commit that will be stripped server-side. Also make two
bookmarks
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - \(1 misses, 0.00% hit ratio\) over [\d.]+s (re)
  $ hg book goodbooktobackup
  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark goodbooktobackup)
  1 files fetched over 1 fetches - \(1 misses, 0.00% hit ratio\) over [\d.]+s (re)
  $ hg book badbooktobackup
  $ mkcommit clientbadcommit
  $ hg log --graph -T '{desc}'
  @  clientbadcommit
  |
  o  committostripsecond
  |
  o  committostripfirst
  |
  o  serverinitialcommit
  
  $ cd ..

Strip commit server-side
  $ cd repo
  $ hg strip 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/48acd0edbb46-9d7996f9-backup.hg (glob)
  $ hg log --graph -T '{desc}'
  @  serverinitialcommit
  
Now do a backup, it should not fail
  $ cd ../client
  $ hg pushbackup
  searching for changes

Now try to restore it from different client. Make sure bookmark
`goodbooktobackup` is restored
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/repo secondclient
  streaming all changes
  2 files to transfer, 268 bytes of data
  transferred 268 bytes in [\d.]+ seconds \([\d.]+ KB/sec\) (re)
  searching for changes
  no changes found
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd secondclient
  $ hg pullbackup --traceback
  pulling from ssh://user@dummy/repo
  no changes found
  $ hg book
     goodbooktobackup          0:22ea264ff89d
