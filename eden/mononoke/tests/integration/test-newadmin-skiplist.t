# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ SKIPLIST_INDEX_BLOBSTORE_KEY='skiplist_from_config' setup_common_config "blob_files"
  $ mononoke_testtool drawdag -R repo << 'EOF'
  > M
  > |
  > L
  > |
  > K
  > |
  > J Q
  > | |
  > I P
  > | |
  > H O
  > | |
  > G N
  > |/
  > F
  > |
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > # bookmark: M main
  > # bookmark: Q other
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  E=3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  F=65174a97145838cb665e879e8cf2be219d324dc498997c1116a1aff67bff4823
  G=45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  H=28b369701701c4bd9ba13828d0f0e963100f58b95d24b6777dcf4d9e288c3c2c
  I=9b594f3cc2c4c628effae4a6e5504591f55775f6033fc64209c57ad80a73d5ee
  J=4ad36771186bef290824f2c8c7eb4aa99ef520094ad531c8c3f199afd325fa2f
  K=5c2b2f43bfcb074fd1255b06257e9ab8335a128a0a24ce6f542c48d809352b75
  L=926b45e5c0826117f4dd61583bc89263939302f9138cd96d5dec928b3fade9a2
  M=7e2f312f229d09b0a17cdf9bdbd08d9a7203e9fc24764e45b70ce14d2079ed2d
  N=cb9fa89467bafb030371c75bd16d9bc60287a893a2c9aa487b2a785671f6ddea
  O=3b2762132b9bc4664398de079b235d3d69810c55d0757d55461f635670465cfd
  P=3def4314cd901d309fbed76f6bf6ca67cb24265691ce18db667aa534d07c6086
  Q=d9228aebada1fb8f2d5d67ed123573780e825e9de56868acf897e3b622661184

Check we can build a new skiplist from scratch [with explicitly
provided blobstore key]
  $ mononoke_newadmin skiplist -R repo -k skiplist_5 build --exponent 2
  *] creating a skiplist from scratch (glob)
  *] built 5 skiplist nodes (glob)

Check we can build a new skiplist based on existing skiplist
  $ mononoke_newadmin skiplist -R repo -k skiplist_5 build --exponent 2
  *] cmap size 5, parent nodecount 0, skip nodecount 5, maxsedgelen 1, maxpedgelen 0 (glob)
  *] skiplist graph has 5 entries (glob)
  *] built 5 skiplist nodes (glob)

Check we can build a new skiplist from scratch [with blobstore
key taken from repo config]
  $ mononoke_newadmin skiplist -R repo build --rebuild --exponent 3
  *] creating a skiplist from scratch (glob)
  *] built 2 skiplist nodes (glob)

Check we can read and display an existing skiplist [with explicitly
provided blobstore key]
  $ mononoke_newadmin skiplist -R repo -k skiplist_5 read
  *] cmap size 5, parent nodecount 0, skip nodecount 5, maxsedgelen 1, maxpedgelen 0 (glob)
  Skiplist graph has 5 entries

Check we can read and display an existing skiplist [with blobstore
key taken from repo config]
  $ mononoke_newadmin skiplist -R repo read --show 7e2f312f229d09b0a17cdf9bdbd08d9a7203e9fc24764e45b70ce14d2079ed2d
  *] cmap size 2, parent nodecount 0, skip nodecount 2, maxsedgelen 1, maxpedgelen 0 (glob)
  Skiplist graph has 2 entries
  7e2f312f229d09b0a17cdf9bdbd08d9a7203e9fc24764e45b70ce14d2079ed2d: Some([(ChangesetId(Blake2(3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd)), Generation(5))])
