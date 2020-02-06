# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# setup repo, usefncache flag for forcing algo encoding run
  $ hg init repo-hg --config format.usefncache=False

# Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

  $ mkcommit secondparent
  $ P2="$(hg log -r . -T '{node}')"
  $ echo $P2
  5b373b3803ae35cbb33299f25faa3db42ec90fc3
  $ hg up -q null
  $ mkcommit firstparent
  $ P1="$(hg log -r . -T '{node}')"
  $ echo $P1
  ce62e57ba2d912b1d003ef77f3e1ea75bada9715
  $ hg merge 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ MERGE="$(hg log -r . -T '{node}')"

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

  $ mononoke_admin convert --from hg --to bonsai "$MERGE"
  * using repo "repo" repoid RepositoryId(0) (glob)
  e3a69d381c99627e69b54ba7c5781a743e7db0008ab6013bacc250e65d6ce37e
  $ mononoke_admin bonsai-fetch "$MERGE" --json 2> /dev/null | jq -r '.["parents"]'
  [
    "ed4388987c94735df7008fddf1ea35b2af059087daf187799423d107f6a5daf9",
    "9ca0c669180ea905f1c7d696a8806f159aef66f5e4ee902df7d4860258af3d80"
  ]

Reverse order of parents
  $ rm -r "$TESTTMP/repo"
  $ rm -r $TESTTMP/monsql
  $ rm -r $TESTTMP/mononoke-config
  $ setup_mononoke_config
  $ echo "$MERGE $P2 $P1" > "$TESTTMP"/fix-parent-order
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo --fix-parent-order "$TESTTMP"/fix-parent-order
  $ mononoke_admin convert --from hg --to bonsai "$MERGE"
  * using repo "repo" repoid RepositoryId(0) (glob)
  604ae7945460476f2a4ab463a8db6d4311213a93f5b2682a8e1139a485610e56
  $ mononoke_admin bonsai-fetch "$MERGE" --json 2> /dev/null | jq -r '.["parents"]'
  [
    "9ca0c669180ea905f1c7d696a8806f159aef66f5e4ee902df7d4860258af3d80",
    "ed4388987c94735df7008fddf1ea35b2af059087daf187799423d107f6a5daf9"
  ]

Specify incorrect parents, make sure blobimport fails
  $ rm -r $TESTTMP/repo
  $ rm -r $TESTTMP/monsql
  $ rm -r $TESTTMP/mononoke-config
  $ setup_mononoke_config
  $ echo "$MERGE $P2 $P2" > "$TESTTMP"/fix-parent-order
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo --fix-parent-order "$TESTTMP"/fix-parent-order &> /dev/null
  [1]
