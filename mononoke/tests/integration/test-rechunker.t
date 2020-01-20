# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# setup config

  $ REPOTYPE="blob_files"
  $ setup_common_config "$REPOTYPE"
  $ cd "$TESTTMP"

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

# Commit files
  $ echo -n f1 > f1
  $ hg commit -Aqm "f1"

  $ hg bookmark master_bookmark -r tip

  $ cd "$TESTTMP"
  $ blobimport repo-hg/.hg repo

  $ FILENODE=$(ls "$TESTTMP/blobstore/blobs" | grep "hgfilenode" | cut -d "." -f 4)

# Check that nothing happens if the filestore is not enabled
  $ mononoke_rechunker "$FILENODE"
  * using repo "repo" repoid RepositoryId(0) (glob)

  $ ls "$TESTTMP/blobstore/blobs" | grep hgfilenode
  blob-repo0000.hgfilenode.sha1.92c09d364cd563132d6eb5f1424ff63523d51f73

# Check that the rechunker complains about an unknown filenode
  $ mononoke_rechunker "ffffffffffffffffffffffffffffffffffffffff"
  * using repo "repo" repoid RepositoryId(0) (glob)
  Error: Mercurial content missing for node ffffffffffffffffffffffffffffffffffffffff (type regular)
  [1]

# Create a new config with the filestore configured
  $ rm -rf "$TESTTMP/mononoke-config"
  $ FILESTORE_CHUNK_SIZE=1 FILESTORE=1 setup_common_config "$REPOTYPE"
  $ cd "$TESTTMP"

  $ mononoke_rechunker "$FILENODE"
  * using repo "repo" repoid RepositoryId(0) (glob)

  $ ls "$TESTTMP/blobstore/blobs" | grep chunk | wc -l
  2
