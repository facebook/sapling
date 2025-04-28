# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

check the read sql path still works with readonly storage
  $ mononoke_admin --with-readonly-storage=true bookmarks -R repo log master_bookmark
  * (master_bookmark) e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2 testmove * (glob)

check that sql writes are blocked by readonly storage
  $ mononoke_admin --with-readonly-storage=true bookmarks -R repo set another_bookmark $B
  Creating publishing bookmark another_bookmark at f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  * While executing InsertBookmarksImpl query (glob)
  
  Caused by:
      0: attempt to write a readonly database
      1: Error code 8: Attempt to write a readonly database
  [1]

