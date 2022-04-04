# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setup_common_config "blob_files"

Check filestore store & fetch

  $ echo foo > "$TESTTMP/blob"

  $ mononoke_newadmin filestore -R repo store "$TESTTMP/blob"
  Wrote 2ff003c268263a870defffe9afdccd3a72e501bbd892f24cac7ca944ac240eb1 (4 bytes)

  $ mononoke_newadmin filestore -R repo fetch -i 2ff003c268263a870defffe9afdccd3a72e501bbd892f24cac7ca944ac240eb1
  foo
