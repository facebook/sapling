  $ . $TESTDIR/library.sh

  $ hook_test_setup $TESTDIR/hooks/limit_filesize.lua conflict_markers PerAddedOrModifiedFile "bypass_commit_string=\"@allow-large-files\""

Small file
  $ hg up -q 0
  $ echo 1 > 1
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev a0c9c5791058 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update
  remote: * DEBG Session with Mononoke started with uuid: * (glob)

Large file
  $ LARGE_CONTENT=11111111111
  $ hg up -q 0
  $ echo "$LARGE_CONTENT" > largefile
  $ hg ci -Aqm largefile
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 328ac95dcdf8 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: * ERRO Command failed, remote: true, error: hooks failed: (glob)
  remote: conflict_markers: File size limit is 10 bytes. You tried to push file largefile that is over the limit, root_cause: ErrorMessage {
  remote:     msg: "hooks failed:\nconflict_markers: File size limit is 10 bytes. You tried to push file largefile that is over the limit"
  remote: }, backtrace: , session_uuid: * (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Bypass
  $ hg commit --amend -m "@allow-large-files"
  saved backup bundle to $TESTTMP/repo2/.hg/strip-backup/328ac95dcdf8-b2f27658-amend.hg (glob)
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev bac6b7a9e627 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

Test with ignored paths
  $ mkdir -p xplat/third-party/yarn/offline-mirror/
  $ echo $LARGE_CONTENT > xplat/third-party/yarn/offline-mirror/flow-bin-1.tgz
  $ hg up -q 0
  $ mkdir fbobjc
  $ echo $LARGE_CONTENT > fbobjc/1.mm
  $ echo $LARGE_CONTENT > 1.graphql
  $ hg commit -Aqm msg
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 0abd25bdf9af to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update
