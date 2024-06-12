# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup a Mononoke repo.

  $ setup_common_config blob_files
  $ cd "$TESTTMP"

Start Mononoke & LFS.

  $ start_and_wait_for_mononoke_server
Create a repo

  $ testtool_drawdag --repo-name repo << EOF
  > A-B-C-D-E-F
  > # bookmark: C main 
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=f41e886d61d03021b73d006acf237244086eb7a5d9c7989e44e59b76d3c3f2b5
  E=3a2426d009267ba6f83945ecb29f63116a21984fb62df772d3bbe0143163b8fd
  F=65174a97145838cb665e879e8cf2be219d324dc498997c1116a1aff67bff4823

  $ sleep 10

Show the phases
From mononoke_admin, we find that A, B and C are public as expected
  $ mononoke_admin phases list-public -c bonsai | sort
  * using repo "repo" repoid RepositoryId(0) (glob)
  aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658

From the source of truth (sqlite), we can also see that A, B and C are public as expected
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select repo_id,hex(cs_id),phase from phases;" | sort
  0|AA53D24251FF3F54B1B2C29AE02826701B2ABEB0079F1BB13B8434B54CD87675|Public
  0|E32A1E342CDB1E38E88466B4C1A01AE9F410024017AA21DC0A1C5DA6B3963BF2|Public
  0|F8C75E41A0C4D29281DF765F39DE47BCA1DCADFDC55ADA4CCC2F6DF567201658|Public

