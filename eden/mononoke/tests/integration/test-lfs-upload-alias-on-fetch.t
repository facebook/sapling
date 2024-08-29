# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# setup config repo

  $ REPOTYPE="blob_files"
  $ export LFS_THRESHOLD="1000"
  $ setup_common_config $REPOTYPE
  $ cd $TESTTMP

# 1. Setup nolfs hg repo, create several commit to it
  $ hginit_treemanifest repo
  $ cd repo

# Commit small file
  $ echo s > smallfile
  $ hg commit -Aqm "add small file"

  $ hg bookmark master_bookmark -r tip

  $ cd ..

# 2. Blobimport hg nolfs to mononoke (blob_files).
#   2.a Motivation: Blobimport for now does not support import of lfs hg repos. (Error with RevlogRepo parsing).
#       So we need to blobimport hg repo without lsf extention.
#   2.b Motivation: For blob_files storage, is because we need to run Mononoke and Mononoke API server.
#       We cannot have 2 processes for 1 RocksDB repo, as RocksDb does not allows us to do that.
#   2.c Still Mononoke config is blobimported to Rocks DB. As Api server and Mononoke server are using them separately.
  $ blobimport repo/.hg repo

# 3. Setup Mononoke. Introduce LFS_THRESHOLD into Mononoke server config.
  $ start_and_wait_for_mononoke_server
# 4. Setup Mononoke API server.
  $ lfs_uri="$(lfs_server)/repo"

# 5. Clone hg nolfs repo to lfs client hg repo. Setup small threshold for large file size.
  $ hg clone -q mono:repo repo-lfs --noupdate
  $ cd repo-lfs
  $ setup_hg_modern_lfs "$lfs_uri" 1000B "$TESTTMP/lfs-cache1"

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

# get smallfile
  $ hg pull -q
  $ hg update -r master_bookmark -q

# 6. Hg push from hg client repo.
  $ LONG=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC
  $ echo $LONG > lfs-largefile
  $ hg commit -Aqm "add lfs-large file"
  $ hg push -r . --to master_bookmark -v
  pushing rev 0db8825b9792 to destination mono:repo bookmark master_bookmark
  searching for changes
  validated revset for rebase
  1 changesets found
  uncompressed size of bundle content:
       205 (changelog)
       282  lfs-largefile
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

# Check that alias.sha1.hgfilenode -> sha256.file_content is not generated
  $ ls $TESTTMP/blobstore/blobs | grep "alias" | sort -h
  blob-repo0000.alias.gitsha1.23c160a91fd3e722a49a86017e103e83e7965af7
  blob-repo0000.alias.gitsha1.b4785957bc986dc39c629de9fac9df46972c00fc
  blob-repo0000.alias.seeded_blake3.3d2f50c6508da9d8025883d80f2b90237dafadafae797d8320822bf8fbd06ac8
  blob-repo0000.alias.seeded_blake3.a718362bb5bc80bc81f8ff7c7016bfd600ef9d82d143e07d2450c79972780d00
  blob-repo0000.alias.sha1.8cfc11d4c1bf45aca9412afc5b95cfd1db83e885
  blob-repo0000.alias.sha1.ded5ba42480fe75dcebba1ce068489ff7be2186a
  blob-repo0000.alias.sha256.cbc80bb5c0c0f8944bf73b3a429505ac5cde16644978bc9a1e74c5755f8ca556
  blob-repo0000.alias.sha256.f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b

  $ cd ..
7. Hg pull from hg client repo.
  $ hg clone -q mono:repo repo-lfs2 --noupdate
  $ cd repo-lfs2
  $ setup_hg_modern_lfs "$lfs_uri" 1000B $TESTTMP/lfs-cache2

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > [remotefilelog]
  > getpackversion = 2
  > EOF

  $ hg pull
  pulling from mono:repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes

  $ ls $TESTTMP/blobstore/blobs | grep "alias.content" | wc -l
  0

  $ hg update -r master_bookmark -v
  resolving manifests
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Check that alias.sha1.hgfilenode -> sha256.file_content is generated
  $ ls $TESTTMP/blobstore/blobs | grep "alias" | sort -h
  blob-repo0000.alias.gitsha1.23c160a91fd3e722a49a86017e103e83e7965af7
  blob-repo0000.alias.gitsha1.b4785957bc986dc39c629de9fac9df46972c00fc
  blob-repo0000.alias.seeded_blake3.3d2f50c6508da9d8025883d80f2b90237dafadafae797d8320822bf8fbd06ac8
  blob-repo0000.alias.seeded_blake3.a718362bb5bc80bc81f8ff7c7016bfd600ef9d82d143e07d2450c79972780d00
  blob-repo0000.alias.sha1.8cfc11d4c1bf45aca9412afc5b95cfd1db83e885
  blob-repo0000.alias.sha1.ded5ba42480fe75dcebba1ce068489ff7be2186a
  blob-repo0000.alias.sha256.cbc80bb5c0c0f8944bf73b3a429505ac5cde16644978bc9a1e74c5755f8ca556
  blob-repo0000.alias.sha256.f11e77c257047a398492d8d6cb9f6acf3aa7c4384bb23080b43546053e183e4b
