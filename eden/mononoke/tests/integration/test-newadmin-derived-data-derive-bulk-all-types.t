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
  $ mononoke_newadmin derived-data -R repo derive-bulk --all-types --start $A --end $C

  $ mononoke_newadmin derived-data -R repo exists -i $C -T blame
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T changeset_info
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T deleted_manifest
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T fastlog
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T filenodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T fsnodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T unodes
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T hgchangesets
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ mononoke_newadmin derived-data -R repo exists -i $C -T skeleton_manifests
  Derived: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

add a more complex graph of changesets:
  $ testtool_drawdag -R repo <<EOF
  > C-D-E
  > # exists: C e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  > EOF
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  E=3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd

  $ mononoke_newadmin derived-data -R repo derive-bulk --all-types --start $C --end $E

  $ mononoke_newadmin derived-data -R repo exists -i $E -T blame
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T changeset_info
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T deleted_manifest
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T fastlog
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T filenodes
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T fsnodes
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T unodes
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T hgchangesets
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  $ mononoke_newadmin derived-data -R repo exists -i $E -T skeleton_manifests
  Derived: 3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
