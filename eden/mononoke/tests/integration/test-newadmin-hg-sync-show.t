# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" quiet default_setup_blobimport

  $ mononoke_newadmin hg-sync -R repo show
  1 (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ mononoke_newadmin hg-sync -R repo last-processed --set 1
  No counter found for repo (0)
  Counter for repo (0) set to 1

  $ mononoke_newadmin hg-sync -R repo show
