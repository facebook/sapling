# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > pushrebase=
  > remotenames=
  > EOF

setup configuration

  $ REPOTYPE="blob_files"
  $ REPOID=0 REPONAME=first_source_repo setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=second_source_repo setup_common_config $REPOTYPE
  $ REPOID=2 REPONAME=hyper_repo setup_common_config $REPOTYPE

  $ cd "$TESTTMP"
  $ hginit_treemanifest first_source_repo
  $ cd first_source_repo
  $ echo first > first
  $ hg add first
  $ hg ci -m 'first commit in first repo'
  $ hg book -r . master_bookmark

  $ cd "$TESTTMP"
  $ hginit_treemanifest second_source_repo
  $ cd second_source_repo
  $ echo second > second
  $ hg add second
  $ hg ci -m 'first commit in second repo'
  $ hg book -r . master_bookmark

blobimport hg servers repos into Mononoke repos
  $ cd "$TESTTMP"
  $ REPOID=0 blobimport first_source_repo/.hg first_source_repo
  $ REPOID=1 blobimport second_source_repo/.hg second_source_repo
  $ REPOID=2 mononoke_hyper_repo_builder master_bookmark main_bookmark add-source-repo --source-repo first_source_repo
  * using repo "first_source_repo" repoid RepositoryId(0) (glob)
  * Reloading redacted config from configerator (glob)
  * using repo "hyper_repo" repoid RepositoryId(2) (glob)
  * Reloading redacted config from configerator (glob)
  * found 1 files in source repo, copying them to hyper repo... (glob)
  * Finished copying (glob)
  * about to create 1 commits (glob)
  * creating * (glob)
  $ REPOID=2 mononoke_hyper_repo_builder master_bookmark main_bookmark add-source-repo --source-repo second_source_repo
  * using repo "second_source_repo" repoid RepositoryId(1) (glob)
  * Reloading redacted config from configerator (glob)
  * using repo "hyper_repo" repoid RepositoryId(2) (glob)
  * Reloading redacted config from configerator (glob)
  * found 1 files in source repo, copying them to hyper repo... (glob)
  * Finished copying (glob)
  * about to create 1 commits (glob)
  * creating * (glob)

  $ cd $TESTTMP
  $ start_and_wait_for_mononoke_server
  $ cd $TESTTMP
  $ REPONAME=hyper_repo hgmn clone --stream mononoke://$(mononoke_address)/hyper_repo hyper_repo --config extensions.treemanifest= --config remotefilelog.reponame=hyper_repo --shallow --config treemanifest.treeonly=true
  streaming all changes
  2 files to transfer, 0 bytes of data
  transferred * bytes in * seconds (* bytes/sec) (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hyper_repo
  $ hgmn up main_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls -R
  .:
  first_source_repo
  second_source_repo
  
  ./first_source_repo:
  first
  
  ./second_source_repo:
  second

push one more commit
  $ cd $TESTTMP
  $ REPONAME=first_source_repo hgmn clone --stream mononoke://$(mononoke_address)/first_source_repo first_source_repo_client --config extensions.treemanifest= --config remotefilelog.reponame=first_source_repo --shallow --config treemanifest.treeonly=true -q
  $ cd $TESTTMP/first_source_repo_client
  $ REPONAME=first_source_repo  hgmn up -q tip
  $ echo newfile > newfile
  $ REPONAME=first_source_repo  hgmn add newfile
  $ REPONAME=first_source_repo hgmn ci -m 'new commit in first repo'
  $ REPONAME=first_source_repo hgmn push -r . --to master_bookmark --config treemanifest.treeonly=true --config extensions.treemanifest=
  pushing rev 1b234100fb5f to destination mononoke://$LOCALIP:$LOCAL_PORT/first_source_repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

now tail it to the hyper repo
  $ REPOID=2 mononoke_hyper_repo_builder master_bookmark main_bookmark tail --once
  * using repo "hyper_repo" repoid RepositoryId(2) (glob)
  * Reloading redacted config from configerator (glob)
  * using repo "*" repoid RepositoryId(*) (glob)
  * Reloading redacted config from configerator (glob)
  *] Initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  *] Done initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  * using repo "*" repoid RepositoryId(*) (glob)
  * Reloading redacted config from configerator (glob)
  *] Initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  *] Done initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  * found 1 commits to sync from first_source_repo repo (glob)
  * preparing 1 commits from Some(ChangesetId(Blake2(16cfc4585314d75292e61e561fa738d7031a21878cb3308d3df815ec2475b72d))) to Some(ChangesetId(Blake2(16cfc4585314d75292e61e561fa738d7031a21878cb3308d3df815ec2475b72d))), repo first_source_repo (glob)
  * started syncing 1 file contents (glob)
  * copied 1 files (glob)
  * synced file contents (glob)

run again, make sure nothing happens
  $ REPOID=2 mononoke_hyper_repo_builder master_bookmark main_bookmark tail --once
  * using repo "hyper_repo" repoid RepositoryId(2) (glob)
  * Reloading redacted config from configerator (glob)
  * using repo "*" repoid RepositoryId(*) (glob)
  * Reloading redacted config from configerator (glob)
  *] Initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  *] Done initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  * using repo "*" repoid RepositoryId(*) (glob)
  * Reloading redacted config from configerator (glob)
  *] Initializing CfgrLiveCommitSyncConfig, repo: * (glob)
  *] Done initializing CfgrLiveCommitSyncConfig, repo: * (glob)

  $ REPOID=2 mononoke_hyper_repo_builder master_bookmark main_bookmark validate main_bookmark |& grep 'all is well'
  * all is well! (glob)
  * all is well! (glob)

  $ cd $TESTTMP/hyper_repo
  $ REPONAME=hyper_repo hgmn pull -q
  $ REPONAME=hyper_repo hgmn up -q tip
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  source-cs-id-first_source_repo=16cfc4585314d75292e61e561fa738d7031a21878cb3308d3df815ec2475b72d
  source-cs-id-second_source_repo=dfc5e41b0552bd0d35d1bfee34aa882dea5a447dcbb086c29ab56f7ce82cdb81
  $ ls -R
  .:
  first_source_repo
  second_source_repo
  
  ./first_source_repo:
  first
  newfile
  
  ./second_source_repo:
  second
