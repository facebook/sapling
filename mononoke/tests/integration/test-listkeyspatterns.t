# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ hg init repo-hg

setup hg server repo
  $ cd repo-hg
  $ setup_hg_server
  $ cd $TESTTMP

setup client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-client --noupdate -q
  $ cd repo-client
  $ setup_hg_client

make a few commits on the server
  $ cd $TESTTMP/repo-hg
  $ hg debugdrawdag <<EOF
  > C E G
  > | | |
  > B D F
  >  \|/
  >   A
  >   |
  >   0
  > EOF

create bookmarks
  $ hg bookmark test/one -r C
  $ hg bookmark test/two -r E
  $ hg bookmark test/three -r G

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke

switch to client and enable inifitepush extension
  $ cd repo-client
  $ setconfig extensions.infinitepush=

match with glob pattern
  $ hgmn book --list-remote test/*
     test/one                  26805aba1e600a82e93661149f2313866a221a7b
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
     test/two                  4b61ff5c62e28cff36152201967390a6e7375604

match with literal pattern
  $ hgmn book --list-remote test
  $ hgmn book --list-remote test/three
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
  $ hgmn book --list-remote test/t*
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
     test/two                  4b61ff5c62e28cff36152201967390a6e7375604

match multiple patterns
  $ hgmn book --list-remote test/one --list-remote test/th*
     test/one                  26805aba1e600a82e93661149f2313866a221a7b
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
