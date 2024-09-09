# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo

setup client repo2
  $ hg clone -q mono:repo repo-client --noupdate
  $ cd repo-client

make a few commits on the server
  $ cd $TESTTMP/repo
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo/.hg repo

Corrupt blobs by replacing one content blob with another
  $ cd blobstore/blobs
  $ cp blob-repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d blob-repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9

start mononoke

  $ start_and_wait_for_mononoke_server

Prefetch should fail with corruption error
  $ cd $TESTTMP/repo-client
  $ hg pull --config ui.disable-stream-clone=true
  pulling from mono:repo
  warning: stream clone is disabled
  requesting all changes
  adding changesets
  adding manifests
  adding file changes

  $ LOG=revisionstore=debug hg prefetch -r ":" 2>&1 | grep "Invalid hash"
  * Errors = 1, Error = Some("005d992c5dcf32993668f7cede29d296c494a5d9 A: Invalid hash: 005d992c5dcf32993668f7cede29d296c494a5d9 (expected) != a2e456504a5e61f763f1a0b36a6c247c7541b2b3 (computed)") (glob)
