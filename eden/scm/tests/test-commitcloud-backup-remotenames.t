#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Setup server
  $ newserver repo
  $ cd ..

Create client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Backup with remotenames enabled. Make sure that it works fine with anon heads
  $ mkcommit remotenamespush
  $ hg --config extensions.remotenames= cloud backup
  backing up stack rooted at f4ca5164f72e
  commitcloud: backed up 1 commit
  remote: pushing 1 commit:
  remote:     f4ca5164f72e  remotenamespush
