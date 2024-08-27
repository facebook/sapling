
#require eden

setup backing repo

  $ setconfig clone.use-rust=True
  $ setconfig checkout.use-rust=True
  $ setconfig experimental.rust-clone-updaterev=True

  $ newclientrepo e1_client test:e1 << 'EOS'
  > E  # bookmark master = E
  > |
  > D
  > |
  > C  # bookmark stable = C
  > |
  > B
  > |
  > A
  > EOS

test eden clone

  $ eden clone $TESTTMP/e1 $TESTTMP/e2
  Cloning new repository at $TESTTMP/e2...
  Success.  Checked out commit 9bc730a1
  $ eden list
  $TESTTMP/e1_client
  $TESTTMP/e2
  $ cd $TESTTMP/e2
  $ ls -a
  .eden
  .hg
  A
  B
  C
  D
  E
  $ hg go $A
  update complete
  $ ls
  A
# Make sure dynamic config doesn't get loaded
  $ [ -f $TESTTMP/e1/.hg/hgrc.dynamic ]
  [1]
  $ [ -f $TESTTMP/e2/.hg/hgrc.dynamic ]
  [1]

test rust clone
  $ cd $TESTTMP
  $ hg config edenfs.command
  $TESTTMP/bin/eden (no-windows !)
  $TESTTMP/bin/eden.bat (windows !)
  $ LOG=cmdclone hg clone eager://$TESTTMP/e1 hemlo --config remotenames.selectivepulldefault='master, stable'
  Cloning e1 into $TESTTMP/hemlo
  TRACE cmdclone: performing rust clone
   INFO clone_metadata{repo="e1"}: cmdclone: enter
  TRACE clone_metadata{repo="e1"}: cmdclone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="e1"}: cmdclone: exit
   INFO get_update_target: cmdclone: enter
   INFO get_update_target: cmdclone: return=Some((HgId("9bc730a19041f9ec7cb33c626e811aa233efb18c"), "master"))
   INFO get_update_target: cmdclone: exit
  $ eden list
  $TESTTMP/e1_client
  $TESTTMP/e2
  $TESTTMP/hemlo
  $ ls -a $TESTTMP/.eden-backing-repos
  e1
  e1_client
  $ ls -a hemlo
  .eden
  .hg
  A
  B
  C
  D
  E
  $ cd hemlo
  $ hg go stable
  update complete
  $ ls
  A
  B
  C
# Make sure dynamic config doesn't get loaded
  $ [ -f $TESTTMP/e1/.hg/hgrc.dynamic ]
  [1]
  $ [ -f $TESTTMP/hemlo/.hg/hgrc.dynamic ]
  [1]

test rust clone with test instead of eager
  $ cd $TESTTMP
  $ hg clone test:e1 testo1 --config remotefilelog.reponame=aname -q
  $ hg clone test:e1 testo2 -q
  $ eden list | grep testo
  $TESTTMP/testo1
  $TESTTMP/testo2
  $ ls -a $TESTTMP/.eden-backing-repos
  aname
  e1
  e1_client

Make sure that --updaterev works on EdenFS
   $ hg clone test:e1 testo3 -u stable
   Cloning e1 into $TESTTMP/testo3
   $ ls testo3
   A
   B
   C
