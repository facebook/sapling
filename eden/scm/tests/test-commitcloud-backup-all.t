#debugruntest-compatible

#require no-eden

  $ setconfig experimental.allowfilepeer=True

  $ enable amend

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setconfig extensions.commitcloud=

  $ setupcommon

  $ hginit server
  $ cd server
  $ setupserver
  $ setconfig remotefilelog.server=true

  $ touch base
  $ hg commit -Aqm base
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/server shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *.*s (glob) (?)
  $ cd shallow

Test pushing of specific sets of commits
  $ hg debugmakepublic .
  $ drawdag <<'EOS'
  >  B  C          
  >  |  |          
  >  A1 A2   D1 D2 D3  E1  E2
  >    \|      \|  |    \ /
  >     .       .  .     .
  >                # amend: A1 -> A2
  >                # amend: D1 -> D2 -> D3
  >                # rebase: E1 -> E2
  > EOS
  $ hg book -r $E1 pinnedvisible --hidden
  $ hg up $D2 -q --hidden

Check backing up top stack commit and mid commit
  $ hg cloud check -r $A1 -r $D2 -r $E1
  * not backed up (glob)
  * not backed up (glob)
  * not backed up (glob)

  $ hg cloud backup --traceback
  commitcloud: head '42952ab62cec' hasn't been uploaded yet
  commitcloud: head '796f1f48de85' hasn't been uploaded yet
  commitcloud: head 'd79a807cba78' hasn't been uploaded yet
  commitcloud: head '4903fdffd9c6' hasn't been uploaded yet
  commitcloud: head 'daeeb2f180d6' hasn't been uploaded yet
  commitcloud: head 'eccc11f58a56' hasn't been uploaded yet
  edenapi: queue 8 commits for upload
  edenapi: queue 8 files for upload
  edenapi: uploaded 8 files
  edenapi: queue 8 trees for upload
  edenapi: uploaded 8 trees
  edenapi: uploaded 8 changesets

  $ hg cloud check -r $A1 -r $D2 -r $E1
  64164d1e0f82f6a670c84728b83061df1b126b5c backed up
  d79a807cba78db45ec042b74da65ebfd6d58eadd backed up
  42952ab62cecf85e36eaab6965b6bf3f5e3e9fe1 backed up
  $ hg cloud check -r $D1 --hidden
  7c8a43610cd6d316f9bec941fa2677e5c7a90bf5 not backed up

Test --force option
  $ hg cloud backup --debug
  commitcloud: nothing to upload

  $ hg cloud backup -f --debug
  commitcloud: head '42952ab62cec' hasn't been uploaded yet
  commitcloud: head '796f1f48de85' hasn't been uploaded yet
  commitcloud: head 'd79a807cba78' hasn't been uploaded yet
  commitcloud: head '4903fdffd9c6' hasn't been uploaded yet
  commitcloud: head 'daeeb2f180d6' hasn't been uploaded yet
  commitcloud: head 'eccc11f58a56' hasn't been uploaded yet
  edenapi: queue 8 commits for upload
  edenapi: queue 8 files for upload
  edenapi: uploaded 8 files
  edenapi: queue 8 trees for upload
  edenapi: uploaded 8 trees
  edenapi: uploading commit '64164d1e0f82f6a670c84728b83061df1b126b5c'...
  edenapi: uploading commit '42952ab62cecf85e36eaab6965b6bf3f5e3e9fe1'...
  edenapi: uploading commit 'd0d71d09c927a6b27ee30a38e721e7d96414cd06'...
  edenapi: uploading commit '796f1f48de85135450ec0786f9f986b72b07be15'...
  edenapi: uploading commit 'd79a807cba78db45ec042b74da65ebfd6d58eadd'...
  edenapi: uploading commit '4903fdffd9c679d607110970995686c46924320c'...
  edenapi: uploading commit 'daeeb2f180d680211d3aa1592558e4eb10459dc0'...
  edenapi: uploading commit 'eccc11f58a56da53c6d0d1fc9d0dfaa396e6f232'...
  edenapi: uploaded 8 changesets
