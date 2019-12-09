#chg-compatible

  $ enable amend
  $ setconfig infinitepushbackup.hostname=testhost

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Setup backupsource
  $ hg clone ssh://user@dummy/repo backupsource -q
  $ cd backupsource

Do a normal backup
  $ mkcommit first
  $ hg cloud backup
  backing up stack rooted at b75a450e74d5
  remote: pushing 1 commit:
  remote:     b75a450e74d5  first
  commitcloud: backed up 1 commit

Make a commit, than prune a commit, than create a bookmark on top of it.
Do a backup and try to restore. Make sure it doesn't fail
  $ hg up -q null
  $ mkcommit tobepruned
  $ hg log -r . -T '{node}\n'
  edb281c9cc7e2e51c382b6f254d1967fdfa5e6ff
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg --hidden book -r edb281c9cc7e2e51c382b6f254d1967fdfa5e6ff newbookonpruned
  $ hg cloud backup
  backing up stack rooted at edb281c9cc7e
  remote: pushing 1 commit:
  remote:     edb281c9cc7e  tobepruned
  commitcloud: backed up 1 commit

Restore the repo
  $ cd ..
  $ hg clone ssh://user@dummy/repo restored -q
  $ cd restored
  $ hg cloud restorebackup
  restoring backup for test from $TESTTMP/backupsource on testhost
  pulling from ssh://user@dummy/repo
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets b75a450e74d5:edb281c9cc7e
