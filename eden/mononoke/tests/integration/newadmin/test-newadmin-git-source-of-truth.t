# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config

  $ mononoke_newadmin git-source-of-truth -R repo show
  No git source of truth config entry found for repo repo

  $ mononoke_newadmin git-source-of-truth -R repo set locked

  $ mononoke_newadmin git-source-of-truth -R repo show
  GitSourceOfTruthConfigEntry { id: RowId(1), repo_id: RepositoryId(0), repo_name: RepositoryName("repo"), source_of_truth: Locked }

  $ mononoke_newadmin git-source-of-truth -R repo set mononoke

  $ mononoke_newadmin git-source-of-truth -R repo show
  GitSourceOfTruthConfigEntry { id: RowId(2), repo_id: RepositoryId(0), repo_name: RepositoryName("repo"), source_of_truth: Mononoke }

  $ mononoke_newadmin git-source-of-truth -R repo set metagit

  $ mononoke_newadmin git-source-of-truth -R repo show
  GitSourceOfTruthConfigEntry { id: RowId(3), repo_id: RepositoryId(0), repo_name: RepositoryName("repo"), source_of_truth: Metagit }
