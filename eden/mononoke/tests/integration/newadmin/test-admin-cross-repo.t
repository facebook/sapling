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
  > EOF

setup configuration

  $ REPOTYPE="blob_files"
  $ REPOID=0 REPONAME=large setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=small setup_common_config $REPOTYPE
  $ setup_commitsyncmap
  $ setup_configerator_configs


  $ testtool_drawdag -R large --print-hg-hashes <<EOF
  > NOT_SYNC_CANDIDATE_HASH
  > |
  > LARGE_EQ_WC_HASH
  > # modify: LARGE_EQ_WC_HASH "fbsource-rest/arvr/1" "1"
  > EOF
  LARGE_EQ_WC_HASH=5fe58731966e85c3b25fbb512863c4f17dd0d364
  NOT_SYNC_CANDIDATE_HASH=f2b4261b972e70228a40203a1bf10b52c8735057

  $ testtool_drawdag -R small --print-hg-hashes <<EOF
  > SMALL_EQ_WC_HASH
  > # modify: LARGE_EQ_WC_HASH "arvr/1" "1"
  > EOF
  SMALL_EQ_WC_HASH=f33b9d91ec2d0c6476e6acd383f02fb4ccb570d2

Try to insert with invalid version name
  $ mononoke_admin cross-repo --source-repo-name large --target-repo-name small insert equivalent-working-copy \
  > --source-commit-id "$LARGE_EQ_WC_HASH" --target-commit-id "$SMALL_EQ_WC_HASH" --version-name invalid  2>&1 | grep 'invalid version'
  * invalid version does not exist (glob)

Now insert with valid version name
  $ mononoke_admin cross-repo --source-repo-name large --target-repo-name small insert equivalent-working-copy \
  > --source-commit-id "$LARGE_EQ_WC_HASH" --target-commit-id "$SMALL_EQ_WC_HASH" --version-name TEST_VERSION_NAME 2>&1 | grep 'successfully inserted'
  * successfully inserted equivalent working copy (glob)
  $ mononoke_admin cross-repo --source-repo-name large --target-repo-name small map -i "$LARGE_EQ_WC_HASH" 2>&1 | grep EquivalentWorking
  EquivalentWorkingCopyAncestor(ChangesetId(Blake2(e306c30d70aad31205b4bbfa9cbf620531da64cbec56593acda219cd85edcd17)), CommitSyncConfigVersion("TEST_VERSION_NAME"))

Now insert not sync candidate entry
  $ mononoke_admin cross-repo --source-repo-name large --target-repo-name small insert not-sync-candidate \
  > --large-commit-id "$NOT_SYNC_CANDIDATE_HASH" --version-name TEST_VERSION_NAME 2>&1 | grep 'successfully inserted'
  * successfully inserted not sync candidate entry (glob)
  $ mononoke_admin cross-repo --source-repo-name large --target-repo-name small map -i "$NOT_SYNC_CANDIDATE_HASH" 2>&1 | grep NotSyncCandidate
  NotSyncCandidate(CommitSyncConfigVersion("TEST_VERSION_NAME"))
