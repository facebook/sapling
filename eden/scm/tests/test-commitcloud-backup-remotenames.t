  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Create client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Backup with remotenames enabled. Make sure that it works fine with anon heads
  $ mkcommit remotenamespush
  $ hg --config extensions.remotenames= cloud backup
  backing up stack rooted at f4ca5164f72e
  remote: pushing 1 commit:
  remote:     f4ca5164f72e  remotenamespush
  commitcloud: backed up 1 commit
