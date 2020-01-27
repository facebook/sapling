# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ DISALLOW_NON_PUSHREBASE=1 ASSIGN_GLOBALREVS=1 EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ hg up -q master_bookmark

Push commit, check a globalrev was assigned
  $ touch file1
  $ hg ci -Aqm commit1
  $ hgmn push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147970

Push another commit, check that the globalrev is incrementing
  $ touch file2
  $ hg ci -Aqm commit2
  $ hgmn push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147971
