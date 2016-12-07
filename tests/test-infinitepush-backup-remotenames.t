  $ . $TESTDIR/require-ext.sh remotenames
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
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
  $ hg --config extensions.remotenames= debugbackup
  searching for changes
  remote: pushing 1 commit:
  remote:     f4ca5164f72e  remotenamespush
