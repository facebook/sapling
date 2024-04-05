#debugruntest-compatible

#require eden

setup backing repo

  $ eagerepo
  $ setconfig clone.use-rust=True
  $ setconfig checkout.use-rust=True
  $ setconfig remotefilelog.reponame=e1

  $ newrepo e1
  $ drawdag << 'EOS'
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

test rust clone
  $ cd $TESTTMP
  $ hg config edenfs.command
  $TESTTMP/bin/eden (no-windows !)
  $TESTTMP/bin/eden.bat (windows !)
  $ setconfig edenfs.backing-repos-dir=$TESTTMP
  $ LOG=cmdclone hg clone --eden test:e1 hemlo --config remotenames.selectivepulldefault='master, stable'
  Cloning e1 into $TESTTMP/hemlo
  TRACE cmdclone: performing rust clone
   INFO get_update_target: cmdclone: enter
   INFO get_update_target: cmdclone: return=Some((HgId("9bc730a19041f9ec7cb33c626e811aa233efb18c"), "master"))
   INFO get_update_target: cmdclone: exit
  $ eden list
  $TESTTMP/e2
  $TESTTMP/hemlo
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
  (activating bookmark stable)
  $ ls
  A
  B
  C
