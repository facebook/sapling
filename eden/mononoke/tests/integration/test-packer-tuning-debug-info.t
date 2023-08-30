# Copyright (c) Meta Platforms, Inc. and affiliates.
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

# Create a single file that contains multiple packing keys
  $ mkdir -p $TESTTMP/pack_key_files4/
  $ echo 'repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd' >> $TESTTMP/pack_key_files4/reporepo.store0.part0.keys.txt
  $ echo 'repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09' >> $TESTTMP/pack_key_files4/reporepo.store0.part0.keys.txt
  $ echo 'repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a' >> $TESTTMP/pack_key_files4/reporepo.store0.part0.keys.txt

# Pack content into a pack
  $ packer --zstd-level 19 --scuba-dataset file://packed.json --keys-dir $TESTTMP/pack_key_files4/ --print-progress --tuning-info-scuba-table "file://${TESTTMP}/tuning_scuba.json"
  File $TESTTMP/pack_key_files4/reporepo.store0.part0.keys.txt, which has 3 lines
  Progress: 100.000%	processing took * (glob)

# Check the tuning log has the following columns
  $ jq -r '.normal * .double | [.blobs_download_time, .compressing_blobs_invidivually_time, .finding_best_packing_strategy_time, .packed_size, .single_compressed_size, .repo_name, .possible_pack_sizes, .uncompressed_size] | @csv' < "${TESTTMP}/tuning_scuba.json" | sort
  *,*,*,*,*,*,*,* (glob)

# Check logging for packed keys (last 3 digits of the compressed size are matched by glob because they can change on zstd crate updates)
  $ jq -r '.int * .normal | [ .blobstore_id, .blobstore_key, .pack_key, .uncompressed_size, .compressed_size ] | @csv' < packed.json | sort | uniq
  0,"repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd","multiblob-ceaffcf37138026deff00f82a0a42640e26a3fa50ec51ae26df9294e10174299.pack",107626,22
  0,"repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a","multiblob-ceaffcf37138026deff00f82a0a42640e26a3fa50ec51ae26df9294e10174299.pack",107640,41??? (glob)
  0,"repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09","multiblob-ceaffcf37138026deff00f82a0a42640e26a3fa50ec51ae26df9294e10174299.pack",107636,26

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
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.seeded_blake3.5b9d867af55bcbe45a5692af84e9225bafa7e9b73184a992ff5fdf03922773d2.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.seeded_blake3.bc1ac23847a32cea3bc9467f102a4da8564a9b40da9797c8edc44597d7f5080e.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.seeded_blake3.bc2a5a60f2a4a3323e8d9962b9072cf90a9a0a290bbb3925dc62b8ff7193b786.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.c714247df863f86d8d0729632ed78ddfcfec10dd.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.e36bdee9c84cf34c336c1d5a30b1b7e54907635c.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha1.f81707fc5f680da4a58d7b51dff36e5fa8ac294f.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.19dac65a9cb4bda4155d0ae8e7ad372af92351620450cea75a858253839386e0.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.85b856bc2313fcddec8464984ab2d384f61625890ee19e4f909dd80ac36e8fd7.pack'
  48 1 '$TESTTMP/blobstore/0/blobs/blob-repo0000.alias.sha256.9b798d4eb3901972c1311a3c6a21480e3f29c8c64cd6bbb81a977ecab56452e3.pack'
