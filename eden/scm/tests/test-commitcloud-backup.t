#debugruntest-compatible

#require no-eden

#inprocess-hg-incompatible
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig experimental.allowfilepeer=True

  $ enable amend
  $ setconfig infinitepushbackup.hostname=testhost

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ enable remotenames

Setup server
  $ newserver repo
  $ setupserver
  $ cd ..

Backup empty repo
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ hg cloud backup
  commitcloud: nothing to upload

Make commit and backup it.
  $ mkcommit commit
  $ hg pushbackup
  commitcloud: head '7e6a6fd9c7c8' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

Make first commit public (by doing push) and then backup new commit
  $ hg debugmakepublic .
  $ hg push --to master --create
  pushing rev 7e6a6fd9c7c8 to destination ssh://user@dummy/repo bookmark master
  searching for changes
  no changes found
  exporting bookmark master
  $ mkcommit newcommit
  $ hg cloud backup
  commitcloud: head '94a60f5ad8b2' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

Create a stack of commits
  $ mkcommit stacked1
  $ mkcommit stacked2

Backup both of them
  $ hg cloud backup
  commitcloud: head 'd4f07a9b37ad' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets

Create one more head and run `hg cloud backup`. Make sure that only new head is pushed
  $ hg up 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit newhead
  $ hg cloud backup
  commitcloud: head '3a30e220fe42' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

Create two more heads and backup them
  $ hg up 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead1
  $ hg up 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead2
  $ hg cloud backup
  commitcloud: head 'f79c5017def3' hasn't been uploaded yet
  commitcloud: head '667453c0787e' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets

Nothing changed, make sure no backup and no connection to the server happens
  $ hg cloud backup --debug
  commitcloud: nothing to upload

Hide a head commit.
  $ hg hide .
  hiding commit 667453c0787e "newhead2"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 7e6a6fd9c7c8
  1 changeset hidden

  $ hg cloud backup --traceback
  commitcloud: nothing to upload

Rebase + backup.
  $ hg log --graph -T '{node} {desc}'
  o  f79c5017def3b9af9928edbb52cc620c74b4b291 newhead1
  │
  │ o  3a30e220fe42e969e34bbe8001b951a20f31f2e8 newhead
  ├─╯
  │ o  d4f07a9b37ad59066d2497f212fb3d3bb8532490 stacked2
  │ │
  │ o  5d3d3ff32f9c60f387f4040c31dbf1ef9df2980b stacked1
  │ │
  │ o  94a60f5ad8b2e007240007edab982b3638a3f38d newcommit
  ├─╯
  @  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 commit
  
  $ hg rebase -s f79c5017de -d 94a60f5a
  rebasing f79c5017def3 "newhead1"

  $ hg cloud backup
  commitcloud: head '8a2d4df2b27f' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset

Make a few public commits. Make sure we don't backup them
  $ hg up 7e6a6fd
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit public1
  $ mkcommit public2
  $ hg debugmakepublic .
  $ hg log -r tip -T '{node}\n'
  e86a4b27b84e4002dfae01369da18a82be010b8e

  $ hg log --graph -T '{node} {desc} {phase}'
  @  e86a4b27b84e4002dfae01369da18a82be010b8e public2 public
  │
  o  d0d4e43f61f9a83b978388bbe0d8271427912e56 public1 public
  │
  │ o  8a2d4df2b27fd146766b821123b3dd48c71e7e64 newhead1 draft
  │ │
  │ │ o  3a30e220fe42e969e34bbe8001b951a20f31f2e8 newhead draft
  ├───╯
  │ │ o  d4f07a9b37ad59066d2497f212fb3d3bb8532490 stacked2 draft
  │ │ │
  │ │ o  5d3d3ff32f9c60f387f4040c31dbf1ef9df2980b stacked1 draft
  │ ├─╯
  │ o  94a60f5ad8b2e007240007edab982b3638a3f38d newcommit draft
  ├─╯
  o  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 commit public
  
  $ hg cloud backup
  commitcloud: nothing to upload

Test cloud check command
  $ mkcommit notbackedup

  $ hg cloud check
  585f89184f72f72e80f17cd586fb5ff16df53f82 not backed up
  $ hg cloud check -r f79c5017def
  f79c5017def3b9af9928edbb52cc620c74b4b291 not backed up
  $ hg cloud check -r . -r f79c5017def
  585f89184f72f72e80f17cd586fb5ff16df53f82 not backed up
  f79c5017def3b9af9928edbb52cc620c74b4b291 not backed up

Delete a commit from the server
  $ rm ../repo/.hg/scratchbranches/index/nodemap/f79c5017def3b9af9928edbb52cc620c74b4b291

Local state still shows it as backed up, but can check the remote
  $ hg cloud check -r "draft()"
  94a60f5ad8b2e007240007edab982b3638a3f38d backed up
  5d3d3ff32f9c60f387f4040c31dbf1ef9df2980b backed up
  d4f07a9b37ad59066d2497f212fb3d3bb8532490 backed up
  3a30e220fe42e969e34bbe8001b951a20f31f2e8 backed up
  8a2d4df2b27fd146766b821123b3dd48c71e7e64 backed up
  585f89184f72f72e80f17cd586fb5ff16df53f82 not backed up
  $ hg cloud check -r "draft()" --remote
  94a60f5ad8b2e007240007edab982b3638a3f38d backed up
  5d3d3ff32f9c60f387f4040c31dbf1ef9df2980b backed up
  d4f07a9b37ad59066d2497f212fb3d3bb8532490 backed up
  3a30e220fe42e969e34bbe8001b951a20f31f2e8 backed up
  8a2d4df2b27fd146766b821123b3dd48c71e7e64 backed up
  585f89184f72f72e80f17cd586fb5ff16df53f82 not backed up

Corrupt backedupheads
  $ cat > .hg/commitcloud/backedupheads.*
  $ hg log -r 'notbackedup()'
  commit:      585f89184f72
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     notbackedup
  
Delete backup state file and try again
  $ rm .hg/commitcloud/backedupheads.*
  $ hg cloud check -r "draft()"
  94a60f5ad8b2e007240007edab982b3638a3f38d backed up
  5d3d3ff32f9c60f387f4040c31dbf1ef9df2980b backed up
  d4f07a9b37ad59066d2497f212fb3d3bb8532490 backed up
  3a30e220fe42e969e34bbe8001b951a20f31f2e8 backed up
  8a2d4df2b27fd146766b821123b3dd48c71e7e64 backed up
  585f89184f72f72e80f17cd586fb5ff16df53f82 not backed up

Hide a commit. Make sure isbackedup still works
  $ hg hide 3a30e220fe42e969e34bbe8001b951a20f31f2e8
  hiding commit 3a30e220fe42 "newhead"
  1 changeset hidden
  $ hg cloud check -r 3a30e220fe42e969e34bbe8001b951a20f31f2e8 --hidden
  3a30e220fe42e969e34bbe8001b951a20f31f2e8 backed up

Run command that creates multiple transactions. Make sure that just one backup is started
  $ cd ..
  $ rm -rf client
  $ hg clone -q test:repo client --config clone.use-rust=true
  $ cd client
  $ hg debugdrawdag -q <<'EOS'
  > C
  > |
  > B D
  > |/
  > A
  > EOS
  $ hg log -r ':' -G -T '{desc} {node}'
  o  C 26805aba1e600a82e93661149f2313866a221a7b
  │
  │ o  D b18e25de2cf5fc4699a029ed635882849e53ef73
  │ │
  o │  B 112478962961147124edd43549aedd1a335e44bf
  ├─╯
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  @  commit 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  

Create logs directory and set correct permissions
  $ setuplogdir

  $ hg cloud backup --config infinitepushbackup.logdir=$TESTTMP/logs
  commitcloud: head 'b18e25de2cf5' hasn't been uploaded yet
  commitcloud: head '26805aba1e60' hasn't been uploaded yet
  edenapi: queue 4 commits for upload
  edenapi: queue 4 files for upload
  edenapi: uploaded 4 files
  edenapi: queue 4 trees for upload
  edenapi: uploaded 4 trees
  edenapi: uploaded 4 changesets
  $ hg cloud check -r ':'
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 backed up
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 backed up
  112478962961147124edd43549aedd1a335e44bf backed up
  b18e25de2cf5fc4699a029ed635882849e53ef73 backed up
  26805aba1e600a82e93661149f2313866a221a7b backed up
  $ hg cloud check -r ':' --json
  {"112478962961147124edd43549aedd1a335e44bf": true, "26805aba1e600a82e93661149f2313866a221a7b": true, "426bada5c67598ca65036d57d9e4b64b0c1ce7a0": true, "7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455": true, "b18e25de2cf5fc4699a029ed635882849e53ef73": true} (no-eol)
  $ hg rebase -s B -d D --config infinitepushbackup.autobackup=True --config infinitepushbackup.logdir=$TESTTMP/logs
  rebasing 112478962961 "B" (B)
  rebasing 26805aba1e60 "C" (C)
  $ waitbgbackup
  $ hg log -r ':' -G -T '{desc} {node}'
  o  C ffeec75ec60331057b875fc5356c57c3ff204500
  │
  o  B 1ef11233b74dfa8b57e8285fd6f546096af8f4c2
  │
  │ x  C 26805aba1e600a82e93661149f2313866a221a7b
  │ │
  o │  D b18e25de2cf5fc4699a029ed635882849e53ef73
  │ │
  │ x  B 112478962961147124edd43549aedd1a335e44bf
  ├─╯
  o  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  
  @  commit 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  
  $ hg cloud check -r 'ffeec75ec + 1ef11233b7'
  ffeec75ec60331057b875fc5356c57c3ff204500 backed up
  1ef11233b74dfa8b57e8285fd6f546096af8f4c2 backed up
  $ hg cloud check -r 'ffeec75ec + 1ef11233b7' --json
  {"1ef11233b74dfa8b57e8285fd6f546096af8f4c2": true, "ffeec75ec60331057b875fc5356c57c3ff204500": true} (no-eol)

Throw in an empty transaction - this should not trigger a backup.
  $ hg debugshell --command "l = repo.lock(); repo.transaction('backup-test')" --config infinitepushbackup.autobackup=True --config infinitepushbackup.logdir=$TESTTMP/logs

Check the logs, make sure just one process was started
  $ cat $TESTTMP/logs/test/*
  
  * starting: *hg cloud upload * (glob)
  commitcloud: head 'ffeec75ec603' hasn't been uploaded yet
  edenapi: queue 2 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 2 changesets

Check if ssh batch mode enables only for background backup and not for foreground
  $ mkcommit ssh1
  $ hg cloud backup -q
  $ mkcommit ssh2
  $ hg cloud backup --background --config infinitepushbackup.logdir=$TESTTMP/logs --config infinitepushbackup.bgdebug=yes
  commitcloud: head 'eec37aac152b' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  $ waitbgbackup

Fail to push a backup by setting fail point:
  $ mkcommit toobig
  $ FAILPOINTS=eagerepo::api::uploadchangesets=return hg cloud backup
  commitcloud: head '73e861ba66d5' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  abort: server responded 500 Internal Server Error for eager://$TESTTMP/repo/upload_changesets: failpoint. Headers: {}
  [255]
  $ hg cloud check -r .
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 not backed up
  $ hg cloud check -r . --json
  {"73e861ba66d5dc1998052f3ae2cf8cf7924ed863": false} (no-eol)

Set the limit back high, and try again
  $ hg cloud backup
  commitcloud: head '73e861ba66d5' hasn't been uploaded yet
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 1 changeset
  $ hg cloud check -r .
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 backed up
  $ hg cloud check -r . --json
  {"73e861ba66d5dc1998052f3ae2cf8cf7924ed863": true} (no-eol)

Remove the backup state file
  $ rm .hg/commitcloud/backedupheads.f6bce706

Remote check still succeeds
  $ hg cloud check -r . --remote
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 backed up

Local check should recover the file
  $ hg cloud check -r .
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 backed up

Check both ways to specify a commit to back up work
  $ hg cloud backup 73e861ba66d5dc1998052f3ae2cf8cf7924ed863
  commitcloud: nothing to upload
  $ hg cloud backup -r 73e861ba66d5dc1998052f3ae2cf8cf7924ed863
  commitcloud: nothing to upload
