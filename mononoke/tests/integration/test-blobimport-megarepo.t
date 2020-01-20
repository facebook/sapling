# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hg init repo-hg --config format.usefncache=False

  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitextras=
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

  $ touch file1
  $ hg add
  adding file1
  $ hg commit -m "adding first commit"

  $ touch file2
  $ hg add
  adding file2
  $ hg commit -m "adding second commit"

Set up the base repo, and a fake source repo
  $ setup_mononoke_config
  $ REPOID=65535 REPONAME=megarepo setup_common_config "blob_files"
  $ cd $TESTTMP

Do the import, cross-check that the mapping is preserved
  $ blobimport repo-hg/.hg repo --source-repo-id 65535
  $ mononoke_admin_sourcerepo --target-repo-id 65535 crossrepo map 59695d47bd01807288e7a7d14aae5e93507c8b4e2b48b8cc4947b18c0e8bf471
  * using repo "repo" repoid RepositoryId(0) (glob)
  * using repo "megarepo" repoid RepositoryId(65535) (glob)
  * changeset resolved as: ChangesetId(Blake2(59695d47bd01807288e7a7d14aae5e93507c8b4e2b48b8cc4947b18c0e8bf471)) (glob)
  Hash 59695d47bd01807288e7a7d14aae5e93507c8b4e2b48b8cc4947b18c0e8bf471 maps to 59695d47bd01807288e7a7d14aae5e93507c8b4e2b48b8cc4947b18c0e8bf471
