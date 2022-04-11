# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" quiet default_setup_blobimport

  $ mononoke_newadmin hg-sync -R repo inspect 1
  Bookmark: master_bookmark
  Created at: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
