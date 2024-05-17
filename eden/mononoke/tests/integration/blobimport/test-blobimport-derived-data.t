# Copyright (c) Meta Platforms, Inc. and affiliates.
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
  > treemanifest=!
  > treemanifestserver=
  > [treemanifest]
  > server=True
  > EOF

  $ touch file1.txt
  $ hg commit -Aqm "commit 1"
  $ hg bookmark master

  $ setup_common_config blob_files

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

  $ mononoke_newadmin derived-data -R repo exists -T fsnodes -B master
  Not Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a
  $ mononoke_newadmin derived-data -R repo exists -T blame -B master
  Not Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a

  $ blobimport --log repo-hg/.hg repo --derived-data-type fsnodes --derived-data-type blame |& grep Deriving
  * Deriving data for: ["fsnodes", "blame", "filenodes"] (glob)

  $ mononoke_newadmin derived-data -R repo exists -T fsnodes -B master
  Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a
  $ mononoke_newadmin derived-data -R repo exists -T blame -B master
  Derived: c5b2b396b5dd503d2b9ca8c19db1cb0e733c48f43c0ae79f2f174866e11ea38a
