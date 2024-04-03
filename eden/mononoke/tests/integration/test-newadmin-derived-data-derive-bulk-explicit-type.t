# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration:
  $ setup_common_config

  $ testtool_drawdag -R repo <<EOF
  > A-B-C
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

derive the first few commits:
  $ mononoke_newadmin derived-data -R repo derive-bulk -T unodes --start $A --end $C

confirm everything was derived:
  $ mononoke_newadmin derived-data -R repo exists -i $C -T blame
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T changeset_info
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T deleted_manifest
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T fastlog
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T filenodes
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T fsnodes
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T unodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T hgchangesets
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T skeleton_manifests
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

derive a few more types:
  $ mononoke_newadmin derived-data -R repo derive-bulk -T fastlog -T hgchangesets --start $A --end $C

  $ mononoke_newadmin derived-data -R repo exists -i $C -T blame
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T changeset_info
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T deleted_manifest
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T fastlog
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T filenodes
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T fsnodes
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T unodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T hgchangesets
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T skeleton_manifests
  Not Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

add a more complex graph of changesets:
  $ testtool_drawdag -R repo <<EOF
  > C-D-E-F-G
  > # exists: C e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  > EOF
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  E=3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  F=65174a97145838cb665e879e8cf2be219d324dc498997c1116a1aff67bff4823
  G=45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97

derive more types, on additional changesets:
  $ mononoke_newadmin derived-data -R repo derive-bulk -T fastlog -T hgchangesets -T unodes --start $E --end $G
  Error: derive exactly batch pre-condition not satisfied: all ancestors' and dependencies' data must already have been derived
  
  Caused by:
      0: a batch ancestor does not have 'fastlog' derived
      1: dependency 'fastlog' of f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5 was not already derived
  [1]

  $ mononoke_newadmin derived-data -R repo derive-bulk -T fastlog -T hgchangesets -T unodes --start $C --end $G

  $ mononoke_newadmin derived-data -R repo exists -i $G -T blame
  Not Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T changeset_info
  Not Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T deleted_manifest
  Not Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T fastlog
  Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T filenodes
  Not Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T fsnodes
  Not Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T unodes
  Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T hgchangesets
  Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
  $ mononoke_newadmin derived-data -R repo exists -i $G -T skeleton_manifests
  Not Derived: 45fbf5ae6a45ed445a0fba067297780f4533d8f92901e499510e1dc268a69f97
