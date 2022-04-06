# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration with some compressable files
  $ PACK_BLOB=0 setup_common_config "blob_files"
  $ cd $TESTTMP
  $ hginit_treemanifest repo-hg-nolfs
  $ cd repo-hg-nolfs
  $ setup_hg_server
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

Run a scrub with the pack logging enabled
  $ mononoke_new_walker -l loaded scrub -q -I deep -i bonsai -i FileContent -b master_bookmark -a all --pack-log-scuba-file pack-info.json 2>&1 | strip_glog
  Seen,Loaded: 7,7

Check logged pack info. Commit time is forced to zero in tests, hence mtime is 0. Expect compressed sizes and no pack_key yet
  $ jq -r '.int * .normal | [ .repo, .chunk_num, .blobstore_key, .node_type, .node_fingerprint, .similarity_key, .mtime, .uncompressed_size, .unique_compressed_size, .pack_key, .ctime] | @csv' < pack-info.json | sort | uniq
  "repo",1,"repo0000.changeset.blake2.22eaf128d2cd64e1e47f9f0f091f835d893415588cb41c66d8448d892bcc0756","Changeset",-2205411614990931422,,0,108,108,,1* (glob)
  "repo",1,"repo0000.changeset.blake2.67472b417c6772992e6c4ef87258527b01a6256ef707a3f9c5fe6bc9679499f8","Changeset",-7389730255194601625,,0,73,73,,1* (glob)
  "repo",1,"repo0000.changeset.blake2.99283342831420aaf2c75c890cf3eb98bb26bf07e94d771cf8239b033ca45714","Changeset",-6187923334023141223,,0,108,108,,1* (glob)
  "repo",1,"repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd","FileContent",975364069869333068,6905401043796602115,0,107626,10*,,1* (glob)
  "repo",1,"repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a","FileContent",1456254697391410303,-6891338160001598946,0,107640,10*,,1* (glob)
  "repo",1,"repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09","FileContent",-7441908177121090870,-6743401566611195657,0,107636,1*,,1* (glob)

Now pack the blobs
  $ (cd blobstore/blobs; ls) | sed -e 's/^blob-//' -e 's/.pack$//' | packer --zstd-level=3

Run a scrub again now the storage is packed
  $ mononoke_new_walker -l loaded scrub -q -I deep -i bonsai -i FileContent -p Changeset --checkpoint-name=bonsai_packed --checkpoint-path=test_sqlite -a all --pack-log-scuba-file pack-info-packed.json 2>&1 | strip_glog
  Seen,Loaded: 6,6
  Deferred: 0

Check logged pack info now the store is packed. Expecting multiple in same pack key
  $ jq -r '.int * .normal | [ .chunk_num, .blobstore_key, .node_type, .node_fingerprint, .similarity_key, .mtime, .uncompressed_size, .unique_compressed_size, .pack_key, .relevant_uncompressed_size, .relevant_compressed_size, .ctime, .checkpoint_name] | @csv' < pack-info-packed.json | sort | uniq
  1,"repo0000.changeset.blake2.22eaf128d2cd64e1e47f9f0f091f835d893415588cb41c66d8448d892bcc0756","Changeset",-2205411614990931422,,0,108,117,"multiblob-e9fc47da6371e725f7d558a0a7abafc029033a5f35de8f7833baffbd66029d25.pack",107748,45*,1*,"bonsai_packed" (glob)
  1,"repo0000.changeset.blake2.67472b417c6772992e6c4ef87258527b01a6256ef707a3f9c5fe6bc9679499f8","Changeset",-7389730255194601625,,0,73,82,"multiblob-e9fc47da6371e725f7d558a0a7abafc029033a5f35de8f7833baffbd66029d25.pack",107713,45*,1*,"bonsai_packed" (glob)
  1,"repo0000.changeset.blake2.99283342831420aaf2c75c890cf3eb98bb26bf07e94d771cf8239b033ca45714","Changeset",-6187923334023141223,,0,108,117,"multiblob-e9fc47da6371e725f7d558a0a7abafc029033a5f35de8f7833baffbd66029d25.pack",107748,45*,1*,"bonsai_packed" (glob)
  1,"repo0000.content.blake2.4caa3d2f7430890df6f5deb3b652fcc88769e3323c0b7676e9771d172a521bbd","FileContent",975364069869333068,6905401043796602115,0,107626,2*,"multiblob-e9fc47da6371e725f7d558a0a7abafc029033a5f35de8f7833baffbd66029d25.pack",21*,4*,1*,"bonsai_packed" (glob)
  1,"repo0000.content.blake2.7f4c8284eea7351488400d6fdf82e1c262a81e20d4abd8ee469841d19b60c94a","FileContent",1456254697391410303,-6891338160001598946,0,107640,4*,"multiblob-e9fc47da6371e725f7d558a0a7abafc029033a5f35de8f7833baffbd66029d25.pack",10*,4*,1*,"bonsai_packed" (glob)
  1,"repo0000.content.blake2.ca629f1bf107b9986c1dcb16aa8aa45bc31ac0a56871c322a6cd16025b0afd09","FileContent",-7441908177121090870,-6743401566611195657,0,107636,2*,"multiblob-e9fc47da6371e725f7d558a0a7abafc029033a5f35de8f7833baffbd66029d25.pack",21*,4*,1*,"bonsai_packed" (glob)
