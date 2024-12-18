# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" quiet default_setup_drawdag

  $ mononoke_admin hg-sync -R repo inspect 1
  Bookmark: master_bookmark
  Created at: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
