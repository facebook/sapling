# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ DISALLOW_NON_PUSHREBASE=1 ALLOW_CASEFOLDING=1 BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ hg up -q master_bookmark

Create commit which only differs in case
  $ touch foo.txt Foo.txt
  $ hg ci -Aqm commit1

Push the commit, showing the flag has worked
  $ hgmn push -q -r . --to master_bookmark || (sed -nr -e 's/, uuid:.*//' -e 's/^.*\] (Caused by.*)/\1/p' "$TESTTMP"/mononoke.out | strip_glog; exit 1)
