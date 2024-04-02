#debugruntest-compatible

#require no-eden

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
  commitcloud: head 'f4ca5164f72e' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
