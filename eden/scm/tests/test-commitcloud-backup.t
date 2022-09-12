#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig format.use-segmented-changelog=true
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ enable amend
  $ setconfig infinitepushbackup.hostname=testhost
  $ disable treemanifest

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ enable remotenames

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Backup empty repo
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ hg cloud backup
  nothing to back up

Make commit and backup it.
  $ mkcommit commit
  $ hg cloud backup
  backing up stack rooted at 7e6a6fd9c7c8
  commitcloud: backed up 1 commit
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  $ scratchnodes
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 168423c30397d95ef5f44d883f0887f0f5be0936

Make first commit public (by doing push) and then backup new commit
  $ hg debugmakepublic .
  $ hg push --to master --create --force
  pushing rev 7e6a6fd9c7c8 to destination ssh://user@dummy/repo bookmark master
  searching for changes
  exporting bookmark master
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ mkcommit newcommit
  $ hg cloud backup
  backing up stack rooted at 94a60f5ad8b2
  commitcloud: backed up 1 commit
  remote: pushing 1 commit:
  remote:     94a60f5ad8b2  newcommit

Create a stack of commits
  $ mkcommit stacked1
  $ mkcommit stacked2

Backup both of them
  $ hg cloud backup
  backing up stack rooted at 94a60f5ad8b2
  commitcloud: backed up 2 commits
  remote: pushing 3 commits:
  remote:     94a60f5ad8b2  newcommit
  remote:     5d3d3ff32f9c  stacked1
  remote:     d4f07a9b37ad  stacked2

Create one more head and run `hg cloud backup`. Make sure that only new head is pushed
  $ hg up 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit newhead
  $ hg cloud backup
  backing up stack rooted at 3a30e220fe42
  commitcloud: backed up 1 commit
  remote: pushing 1 commit:
  remote:     3a30e220fe42  newhead

Create two more heads and backup them
  $ hg up 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead1
  $ hg up 7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead2
  $ hg cloud backup
  backing up stack rooted at f79c5017def3
  backing up stack rooted at 667453c0787e
  commitcloud: backed up 2 commits
  remote: pushing 1 commit:
  remote:     f79c5017def3  newhead1
  remote: pushing 1 commit:
  remote:     667453c0787e  newhead2

Nothing changed, make sure no backup and no connection to the server happens
  $ hg cloud backup --debug
  nothing to back up

Hide a head commit.
  $ hg hide .
  hiding commit 667453c0787e "newhead2"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 7e6a6fd9c7c8
  1 changeset hidden

  $ hg cloud backup --traceback
  nothing to back up

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
  backing up stack rooted at 94a60f5ad8b2
  commitcloud: backed up 1 commit
  remote: pushing 2 commits:
  remote:     94a60f5ad8b2  newcommit
  remote:     8a2d4df2b27f  newhead1

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
  nothing to back up

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
  $ hg clone --no-shallow ssh://user@dummy/repo client -q
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
  backing up stack rooted at 426bada5c675
  commitcloud: backed up 4 commits
  remote: pushing 4 commits:
  remote:     426bada5c675  A
  remote:     b18e25de2cf5  D
  remote:     112478962961  B
  remote:     26805aba1e60  C
  $ hg cloud check -r ':'
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 backed up
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 backed up
  112478962961147124edd43549aedd1a335e44bf backed up
  b18e25de2cf5fc4699a029ed635882849e53ef73 backed up
  26805aba1e600a82e93661149f2313866a221a7b backed up
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

Throw in an empty transaction - this should not trigger a backup.
  $ hg debugshell --command "l = repo.lock(); repo.transaction('backup-test')" --config infinitepushbackup.autobackup=True --config infinitepushbackup.logdir=$TESTTMP/logs

Check the logs, make sure just one process was started
  $ cat $TESTTMP/logs/test/*
  
  * starting: hg cloud backup * (glob)
  backing up stack rooted at 426bada5c675
  commitcloud: backed up 2 commits
  remote: pushing 4 commits:
  remote:     426bada5c675  A
  remote:     b18e25de2cf5  D
  remote:     1ef11233b74d  B
  remote:     ffeec75ec603  C

Check if ssh batch mode enables only for background backup and not for foreground
  $ mkcommit ssh1
  $ hg cloud backup -q
  $ mkcommit ssh2
  $ hg cloud backup --background --config infinitepushbackup.logdir=$TESTTMP/logs --config infinitepushbackup.bgdebug=yes
  $ waitbgbackup

Fail to push a backup by setting the server maxbundlesize very low
  $ cp ../repo/.hg/hgrc $TESTTMP/server-hgrc.bak
  $ cat >> ../repo/.hg/hgrc << EOF
  > [infinitepush]
  > maxbundlesize = 0
  > EOF
  $ mkcommit toobig
  $ hg cloud backup
  backing up stack rooted at 0ff831d99e2e
  remote: pushing 3 commits:
  remote:     0ff831d99e2e  ssh1
  remote:     eec37aac152b  ssh2
  remote:     73e861ba66d5  toobig
  push failed: bundle is too big: 1488 bytes. max allowed size is 0 MB
  retrying push with discovery
  searching for changes
  remote: pushing 3 commits:
  remote:     0ff831d99e2e  ssh1
  remote:     eec37aac152b  ssh2
  remote:     73e861ba66d5  toobig
  push of head 73e861ba66d5 failed: bundle is too big: 1488 bytes. max allowed size is 0 MB
  commitcloud: failed to back up 1 commit
  [2]
  $ hg cloud check -r .
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 not backed up
  $ scratchnodes | grep 73e861ba66d5dc1998052f3ae2cf8cf7924ed863
  [1]

Set the limit back high, and try again
  $ mv $TESTTMP/server-hgrc.bak ../repo/.hg/hgrc
  $ hg cloud backup
  backing up stack rooted at 0ff831d99e2e
  commitcloud: backed up 1 commit
  remote: pushing 3 commits:
  remote:     0ff831d99e2e  ssh1
  remote:     eec37aac152b  ssh2
  remote:     73e861ba66d5  toobig
  $ hg cloud check -r .
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 backed up
  $ scratchnodes | grep 73e861ba66d5dc1998052f3ae2cf8cf7924ed863
  73e861ba66d5dc1998052f3ae2cf8cf7924ed863 1b5db94fc7daec8da5284b7b989fff125cb6f35b

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
  nothing to back up
  $ hg cloud backup -r 73e861ba66d5dc1998052f3ae2cf8cf7924ed863
  nothing to back up
