# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" quiet default_setup

  $ mononoke_admin hg-sync-bundle inspect 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  Bookmark: master_bookmark
  * Log entry is a bookmark creation. (glob)
  === To ===
  BonsaiChangesetId: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 
  Author: test 
  Message: C 
  FileChanges:
  	 ADDED/MODIFIED: C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
