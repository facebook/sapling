# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig workingcopy.ruststatus=False

# setup repo, usefncache flag for forcing algo encoding run
  $ hg init repo-hg --config format.usefncache=False

# Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=!
  > treemanifestserver=
  > [treemanifest]
  > server=True
  > EOF

EOF
# Push single empty commit
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg rm 1
  $ hg commit --amend
  $ hg log -r . -T '{manifest}'
  0000000000000000000000000000000000000000 (no-eol)

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters";
  0|highest-imported-gen-num|1
