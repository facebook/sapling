# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hg init repo-hg --config format.usefncache=False

# Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

  $ touch file1.txt
  $ hg commit -Aqm "commit 1"
  $ hg bookmark master

  $ setup_mononoke_config blob_files

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

  $ mononoke_admin derived-data exists fsnodes master 2> /dev/null
  Not Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a
  $ mononoke_admin derived-data exists blame master 2> /dev/null
  Not Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a

  $ blobimport --log repo-hg/.hg repo --derived-data-type fsnodes --derived-data-type blame |& grep Deriving
  * Deriving data for: ["fsnodes", "blame"] (glob)

  $ mononoke_admin derived-data exists fsnodes master 2> /dev/null
  Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a
  $ mononoke_admin derived-data exists blame master 2> /dev/null
  Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a
