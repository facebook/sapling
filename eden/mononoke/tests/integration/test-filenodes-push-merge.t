# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2


Creating a merge commit
  $ hg up -q null
  $ echo 1 > tomerge
  $ hg -q addremove
  $ hg ci -m 'tomerge'
  $ NODE="$(hg log -r . -T '{node}')"
  $ hg up -q master_bookmark
  $ hg merge -q -r "$NODE"
  $ hg ci -m 'merge'

Pushing a merge
  $ hg push -r . --to master_bookmark
  pushing rev 7f168f25ab51 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ mononoke_admin filenodes validate "$(hg log -r master_bookmark -T '{node}')"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)
