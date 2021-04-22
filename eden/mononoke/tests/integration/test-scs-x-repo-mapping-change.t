# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup config repo:
  $ cd "$TESTTMP"
  $ setup_configerator_configs

  $ INFINITEPUSH_ALLOW_WRITES=true \
  >   INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' \
  >   create_large_small_repo
  Setting up hg server repos
  Blobimporting them
  Adding synced mapping entry
  $ cd "$TESTTMP/small-hg-client"
  $ enable infinitepush pushrebase remotenames
  $ setconfig infinitepush.server=false infinitepush.branchpattern="re:scratch/.+"
  $ cd "$TESTTMP/large-hg-client"
  $ enable infinitepush pushrebase remotenames
  $ setconfig infinitepush.server=false infinitepush.branchpattern="re:scratch/.+"

start SCS server and mononoke
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "allow_change_xrepo_mapping_extra": true
  >   }
  > }
  > EOF
  $ start_and_wait_for_scs_server
  $ start_large_small_repo
  Starting Mononoke server

make some simple requests that we can use to check scuba logging

-- sync a commit which changes the mapping used to rewrite a commit
  $ update_commit_sync_map_first_option

  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn up -q master_bookmark 
  $ REPONAME=large-mon hgmn mv -q smallrepofolder smallrepofolder_after
  $ hg ci -Aqm "commit which changes the mapping" --extra "change-xrepo-mapping-to-version=new_version"
  $ echo new_content > smallrepofolder_after/file.txt
  $ hg ci -Aqm 'commit after mapping change'
  $ REPONAME=large-mon hgmn push -qr . --to scratch/mapping_change --create

  $ SMALL_REPO_COMMIT="$(scsc xrepo-lookup \
  > --source-repo=large-mon \
  > --target-repo=small-mon \
  > --bookmark=scratch/mapping_change)"
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -qr "$SMALL_REPO_COMMIT"
  $ REPONAME=small-mon hgmn up -q "$SMALL_REPO_COMMIT"
  $ cat ./file.txt
  new_content
  $ mononoke_admin_source_target 0 1 crossrepo map scratch/mapping_change
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  * using repo "large-mon" repoid RepositoryId(0) (glob)
  * using repo "small-mon" repoid RepositoryId(1) (glob)
  * changeset resolved as: ChangesetId(Blake2(99422dd32c96c129e248b13139d0235afb2d7e40399b6eeb41e0e68dcde33676)) (glob)
  RewrittenAs([(ChangesetId(Blake2(e56f1455ae7d43b9972781881be8c764e37d414068a10bbaee0ff99fb51ff633)), CommitSyncConfigVersion("new_version"))])
