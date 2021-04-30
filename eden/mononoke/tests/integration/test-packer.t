# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

# setup config repo

  $ REPOTYPE="blob_files"
  $ MULTIPLEXED=1
  $ PACK_BLOB=1
  $ setup_common_config $REPOTYPE
  $ cd $TESTTMP

  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server

# Commit files
  $ cp "${TEST_FIXTURES}/raw_text.txt" f1
  $ 
  $ hg commit -Aqm "f1"
  $ cp f1 f2
  $ echo "More text" >> f2
  $ hg commit -Aqm "f2"
  $ cp f1 f3
  $ echo "Yet more text" >> f3
  $ hg commit -Aqm "f3"

  $ hg bookmark master_bookmark -r tip

  $ cd ..

  $ blobimport repo-hg-nolfs/.hg repo

# Get the space consumed by the content as-is
  $ stat -c '%s %h %N' $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.* | sort -n
  107639 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd.pack'
  107649 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09.pack'
  107653 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a.pack'
# Pack content individually, to show recompression effect
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09
  > EOF
  $ packer --zstd-level 10 --inner-blobstore-id 0 << EOF
  > repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a
  > EOF

# Get the space consumed by the recompressed files, and see hardlink count of 1 (individual files)
  $ stat -c '%s %h %N' $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.* | sort -n
  * 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd.pack' (glob)
  * 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09.pack' (glob)
  * 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a.pack' (glob)

# Confirm that filenames are not present in single compressed blobs
  $ grep --files-without-match 'content.blake2.' $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.* | sort
  $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd.pack
  $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a.pack
  $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09.pack

# Pack content into a pack
  $ packer --zstd-level 19 --inner-blobstore-id 0 << EOF
  > repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd
  > repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09
  > repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a
  > EOF

# Get the space consumed by the packed files, and see hardlink count of 3, showing that they're in one pack
  $ stat -c '%s %h %N' $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.* | sort -n
  * 3 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd.pack' (glob)
  * 3 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a.pack' (glob)
  * 3 '$TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09.pack' (glob)

# Confirm that filenames are present in packs
  $ grep --files-with-matches 'content.blake2.' $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.* | sort
  $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd.pack
  $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a.pack
  $TESTTMP/blobstore/0/blobs/blob-repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09.pack

# Get the space consumed by aliases - this should be small
  $ stat -c '%s %h %N' $TESTTMP/blobstore/0/blobs/blob-repo0000.alias.* | sort -n
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.gitsha1.3df6501a508835a9bc5098b7659c34f97c31c955.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.gitsha1.95a55295a562971d9b7a228a27865342998e0fc6.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.gitsha1.db001d5a57109687474038c8d819062057ce4e23.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.c714247df863f86d8d0729632ed78ddfcfec10dd.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.e36bdee9c84cf34c336c1d5a30b1b7e54907635c.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.f81707fc5f680da4a58d7b51dff36e5fa8ac294f.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.19dac65a9cb4bda4155d0ae8e7ad372af92351620450cea75a858253839386e0.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.85b856bc2313fcddec8464984ab2d384f61625890ee19e4f909dd80ac36e8fd7.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.9b798d4eb3901972c1311a3c6a21480e3f29c8c64cd6bbb81a977ecab56452e3.pack'

# Attempt to pack aliases together
  $ packer --zstd-level 19 --inner-blobstore-id 0 << EOF
  > repo0000.alias.gitsha1.3df6501a508835a9bc5098b7659c34f97c31c955
  > repo0000.alias.gitsha1.95a55295a562971d9b7a228a27865342998e0fc6
  > repo0000.alias.gitsha1.db001d5a57109687474038c8d819062057ce4e23
  > repo0000.alias.sha1.c714247df863f86d8d0729632ed78ddfcfec10dd
  > repo0000.alias.sha1.e36bdee9c84cf34c336c1d5a30b1b7e54907635c
  > repo0000.alias.sha1.f81707fc5f680da4a58d7b51dff36e5fa8ac294f
  > repo0000.alias.sha256.19dac65a9cb4bda4155d0ae8e7ad372af92351620450cea75a858253839386e0
  > repo0000.alias.sha256.85b856bc2313fcddec8464984ab2d384f61625890ee19e4f909dd80ac36e8fd7
  > repo0000.alias.sha256.9b798d4eb3901972c1311a3c6a21480e3f29c8c64cd6bbb81a977ecab56452e3
  > EOF

# Show that they're not packed (hardlink count of 1)
  $ stat -c '%s %h %N' $TESTTMP/blobstore/0/blobs/blob-repo0000.alias.* | sort -n
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.gitsha1.3df6501a508835a9bc5098b7659c34f97c31c955.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.gitsha1.95a55295a562971d9b7a228a27865342998e0fc6.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.gitsha1.db001d5a57109687474038c8d819062057ce4e23.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.c714247df863f86d8d0729632ed78ddfcfec10dd.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.e36bdee9c84cf34c336c1d5a30b1b7e54907635c.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.f81707fc5f680da4a58d7b51dff36e5fa8ac294f.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.19dac65a9cb4bda4155d0ae8e7ad372af92351620450cea75a858253839386e0.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.85b856bc2313fcddec8464984ab2d384f61625890ee19e4f909dd80ac36e8fd7.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.9b798d4eb3901972c1311a3c6a21480e3f29c8c64cd6bbb81a977ecab56452e3.pack'
