# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

Setup repo config (we use blob_files to share across Mononoke and API Server):
  $ LFS_THRESHOLD="1000" LFS_ROLLOUT_PERCENTAGE="0" setup_common_config "blob_files"
  $ cd $TESTTMP

Setup hg repo, create a commit there. No LFS blobs yet.
  $ hginit_treemanifest repo
  $ cd repo

Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"
  $ hg bookmark master_bookmark -r tip
  $ cd ..

Blobimport the hg repo to Mononoke
  $ blobimport repo/.hg repo

Start Mononoke with LFS enabled.
  $ start_and_wait_for_mononoke_server
Start Mononoke API server, to serve LFS blobs
  $ lfs_uri="$(lfs_server)/repo"

Create a new client repository. Enable LFS there.
  $ hg clone -q mono:repo repo-lfs --noupdate
  $ cd repo-lfs
  $ setup_hg_modern_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache1"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

Update in the client repo
  $ hg pull -q
  $ hg update -r master_bookmark -q

Perform LFS push
  $ LONG="$(yes A 2>/dev/null | head -c 2000)"
  $ echo "$LONG" > lfs-largefile
  $ hg commit -Aqm "add lfs-large files"
  $ hg push -r . --to master_bookmark -v
  pushing rev 99262937f158 to destination mono:repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       206 (changelog)
       282  lfs-largefile
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Create a new client repository, using getpack (with its own cachepath).
  $ hg clone -q mono:repo repo-lfs2 --noupdate
  $ cd repo-lfs2
  $ setup_hg_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache2"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > [remotefilelog]
  > fetchpacks = True
  > getpackversion = 2
  > cachepath=$TESTTMP/cachepath-alt
  > EOF

  $ hg pull -q

Make sure lfs is not used during update
  $ hg update -r master_bookmark -v
  resolving manifests
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Create a new client repository, using getpack (with its own cachepath).
  $ hg clone -q mono:repo repo-lfs3 --noupdate
  $ cd repo-lfs3
  $ setup_hg_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache3"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > [remotefilelog]
  > fetchpacks = True
  > getpackversion = 2
  > lfs = True
  > cachepath=$TESTTMP/cachepath-alt2
  > EOF

  $ hg pull -q

Now set wantslfspointers, make sure we download lfs pointers
  $ hg update -r master_bookmark -v --config lfs.wantslfspointers=True
  resolving manifests
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
