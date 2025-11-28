# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration with some compressable files.  3 way multiplex with first two stores packed
  $ MULTIPLEXED=2 PACK_BLOB=1 setup_common_config "blob_files"
  $ cd "$TESTTMP"

  $ RAW_CONTENT="$(cat "${TEST_FIXTURES}/raw_text.txt")"
  $ testtool_drawdag -R repo <<EOF
  > A-B-C
  > # modify: A f1 "\$RAW_CONTENT"
  > # modify: B f2 "\$RAW_CONTENT\nMore text"
  > # modify: C f3 "\$RAW_CONTENT\nYet more text"
  > # bookmark: C master_bookmark
  > EOF
  A=4ba27f41cc326890da6e254bb824c6f1724378575bac5a307afc6b544fe8a2a1
  B=45974e8b9bffc5370ded8ba277290438034dd49fdfde3c30983a690997def64f
  C=77abfb154c8e8abf35510001ddbd04041c7272f0cb20b89f9d1657f1e8c65e15

Set up the key file for packing
  $ mkdir -p $TESTTMP/pack_key_files_0/
  $ (cd blobstore/0/blobs; ls) | sed -e 's/^blob-//' -e 's/.pack$//' >> $TESTTMP/pack_key_files_0/reporepo.store0.part1.keys.txt
  $ mkdir -p $TESTTMP/pack_key_files_1/
  $ (cd blobstore/0/blobs; ls) | sed -e 's/^blob-//' -e 's/.pack$//' >> $TESTTMP/pack_key_files_1/reporepo.store1.part1.keys.txt

Pack the blobs in the two packed stores differently
  $ packer --zstd-level=3 --keys-dir $TESTTMP/pack_key_files_0/ --tuning-info-scuba-log-file "${TESTTMP}/tuning_scuba.json"
  $ packer --zstd-level=19 --keys-dir $TESTTMP/pack_key_files_1/ --tuning-info-scuba-log-file "${TESTTMP}/tuning_scuba.json"

Run a scrub, need a scrub action to put ScrubBlobstore in the stack, which is necessary to make sure all the inner stores of the multiplex are read
  $ mononoke_walker --blobstore-scrub-action=ReportOnly scrub -q -I deep -i bonsai -i FileContent -b master_bookmark -a all --pack-log-scuba-file pack-info-packed.json 2>&1 | grep -vE "(Bytes|Walked)/s"
  [INFO] Walking edge types [BookmarkToChangeset, ChangesetToBonsaiParent, ChangesetToFileContent]
  [INFO] Walking node types [Bookmark, Changeset, FileContent]
  [INFO] [walker scrub{repo=repo}] Seen,Loaded: 10,10

Check logged pack info now the store is packed. Expecting to see two packed stores and one unpacked
  $ jq -r '.int * .normal | [ .blobstore_id, .chunk_num, .blobstore_key, .node_type, .node_fingerprint, .similarity_key, .mtime, .uncompressed_size, .unique_compressed_size, .pack_key, .relevant_uncompressed_size, .relevant_compressed_size ] | @csv' < pack-info-packed.json | sort | uniq
  0,1,"repo0000.changeset.blake2.45974e8b9bffc5370ded8ba277290438034dd49fdfde3c30983a690997def64f","Changeset",4018899286020233029,,0,153,153,,,
  0,1,"repo0000.changeset.blake2.4ba27f41cc326890da6e254bb824c6f1724378575bac5a307afc6b544fe8a2a1","Changeset",-8041121281816419765,,0,118,118,,,
  0,1,"repo0000.changeset.blake2.77abfb154c8e8abf35510001ddbd04041c7272f0cb20b89f9d1657f1e8c65e15","Changeset",-4644743608241771657,,0,153,153,,,
  0,1,"repo0000.content.blake2.264529c5bd692bd8e876be6132600fe7d04fc3425463b5efbff1eb7b07cfc64e","FileContent",-2870084073741007578,-6891338160001598946,0,29,29,,,
  0,1,"repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","FileContent",-5148279705570089387,1118993463608461201,0,4,4,,,
  0,1,"repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","FileContent",4679342931123202697,2609666726012457483,0,4,4,,,
  0,1,"repo0000.content.blake2.c5685dc4c9a8e8595d425734b2a2cb968848ee830f18bd9b958fdc66d56fe844","FileContent",6478613648508807365,6905401043796602115,0,15,15,,,
  0,1,"repo0000.content.blake2.cd9c13912d0aabf8cea164a70be856433551128f982986230980c9b56d222b16","FileContent",-528317340462113587,-6743401566611195657,0,25,25,,,
  0,1,"repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","FileContent",-771035176585636117,595132756262828846,0,4,4,,,
  1,1,"repo0000.changeset.blake2.45974e8b9bffc5370ded8ba277290438034dd49fdfde3c30983a690997def64f","Changeset",4018899286020233029,,0,153,153,,,
  1,1,"repo0000.changeset.blake2.4ba27f41cc326890da6e254bb824c6f1724378575bac5a307afc6b544fe8a2a1","Changeset",-8041121281816419765,,0,118,118,,,
  1,1,"repo0000.changeset.blake2.77abfb154c8e8abf35510001ddbd04041c7272f0cb20b89f9d1657f1e8c65e15","Changeset",-4644743608241771657,,0,153,153,,,
  1,1,"repo0000.content.blake2.264529c5bd692bd8e876be6132600fe7d04fc3425463b5efbff1eb7b07cfc64e","FileContent",-2870084073741007578,-6891338160001598946,0,29,29,,,
  1,1,"repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","FileContent",-5148279705570089387,1118993463608461201,0,4,4,,,
  1,1,"repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","FileContent",4679342931123202697,2609666726012457483,0,4,4,,,
  1,1,"repo0000.content.blake2.c5685dc4c9a8e8595d425734b2a2cb968848ee830f18bd9b958fdc66d56fe844","FileContent",6478613648508807365,6905401043796602115,0,15,15,,,
  1,1,"repo0000.content.blake2.cd9c13912d0aabf8cea164a70be856433551128f982986230980c9b56d222b16","FileContent",-528317340462113587,-6743401566611195657,0,25,25,,,
  1,1,"repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","FileContent",-771035176585636117,595132756262828846,0,4,4,,,
  2,1,"repo0000.changeset.blake2.45974e8b9bffc5370ded8ba277290438034dd49fdfde3c30983a690997def64f","Changeset",4018899286020233029,,0,153,,,,
  2,1,"repo0000.changeset.blake2.4ba27f41cc326890da6e254bb824c6f1724378575bac5a307afc6b544fe8a2a1","Changeset",-8041121281816419765,,0,118,,,,
  2,1,"repo0000.changeset.blake2.77abfb154c8e8abf35510001ddbd04041c7272f0cb20b89f9d1657f1e8c65e15","Changeset",-4644743608241771657,,0,153,,,,
  2,1,"repo0000.content.blake2.264529c5bd692bd8e876be6132600fe7d04fc3425463b5efbff1eb7b07cfc64e","FileContent",-2870084073741007578,-6891338160001598946,0,29,,,,
  2,1,"repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","FileContent",-5148279705570089387,1118993463608461201,0,4,,,,
  2,1,"repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","FileContent",4679342931123202697,2609666726012457483,0,4,,,,
  2,1,"repo0000.content.blake2.c5685dc4c9a8e8595d425734b2a2cb968848ee830f18bd9b958fdc66d56fe844","FileContent",6478613648508807365,6905401043796602115,0,15,,,,
  2,1,"repo0000.content.blake2.cd9c13912d0aabf8cea164a70be856433551128f982986230980c9b56d222b16","FileContent",-528317340462113587,-6743401566611195657,0,25,,,,
  2,1,"repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","FileContent",-771035176585636117,595132756262828846,0,4,,,,
