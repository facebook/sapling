# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_sqlite"
  $ mononoke_testtool drawdag -R repo --derive-all <<'EOF'
  > A-B-C
  > # bookmark: C main
  > EOF
  *] Reloading redacted config from configerator (glob)
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

  $ mononoke_newadmin fetch -R repo -i e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  *] Reloading redacted config from configerator (glob)
  BonsaiChangesetId: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  Author: author
  Message: C
  FileChanges:
  	 ADDED/MODIFIED: C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  
  $ mononoke_newadmin fetch -R repo -i aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675 --json | jq -S .
  *] Reloading redacted config from configerator (glob)
  {
    "author": "author",
    "author_date": "1970-01-01T00:00:00+00:00",
    "changeset_id": "aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675",
    "committer": null,
    "committer_date": null,
    "extra": {},
    "file_changes": {
      "A": {
        "Change": {
          "copy_from": null,
          "inner": {
            "content_id": "eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9",
            "file_type": "Regular",
            "size": 1
          }
        }
      }
    },
    "message": "A",
    "parents": []
  }

  $ mononoke_newadmin fetch -R repo -B main -p ""
  *] Reloading redacted config from configerator (glob)
  A 005d992c5dcf32993668f7cede29d296c494a5d9 regular
  B 35e7525ce3a48913275d7061dd9a867ffef1e34d regular
  C a2e456504a5e61f763f1a0b36a6c247c7541b2b3 regular
