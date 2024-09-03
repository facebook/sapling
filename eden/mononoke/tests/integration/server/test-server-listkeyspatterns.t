# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo
  $ hginit_treemanifest repo

setup hg server repo
  $ cd repo
  $ cd $TESTTMP

setup client repo
  $ hg clone -q mono:repo repo-client --noupdate
  $ cd repo-client

make a few commits on the server
  $ cd $TESTTMP/repo
  $ hg debugdrawdag <<'EOF'
  > C E G
  > | | |
  > B D F
  >  \|/
  >   A
  > EOF

create bookmarks
  $ hg bookmark test/one -r C
  $ hg bookmark test/two -r E
  $ hg bookmark test/three -r G
  $ hg bookmark special/__test__ -r B
  $ hg bookmark special/xxtestxx -r D

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
switch to client and enable infinitepush extension
  $ cd repo-client
  $ setconfig extensions.infinitepush= extensions.commitcloud=

match with glob pattern
  $ hg book --list-remote test/*
     test/one                  26805aba1e600a82e93661149f2313866a221a7b
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
     test/two                  4b61ff5c62e28cff36152201967390a6e7375604

match with literal pattern
  $ hg book --list-remote test
  $ hg book --list-remote test/three
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
  $ hg book --list-remote test/t*
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6
     test/two                  4b61ff5c62e28cff36152201967390a6e7375604

match multiple patterns
  $ hg book --list-remote test/one --list-remote test/th*
     test/one                  26805aba1e600a82e93661149f2313866a221a7b
     test/three                051cf22dff5ca70a5ba3d06d1f9dd08407dfd1a6

match with SQL wildcards doesn't match arbitrary things (should match nothing)
  $ hg book --list-remote t__t/*

match with SQL wildcards does match things with those characters
  $ hg book --list-remote special/__test*
     special/__test__          112478962961147124edd43549aedd1a335e44bf
