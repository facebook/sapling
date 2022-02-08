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
  > # extra: A example_extra "123\xff"
  > EOF
  *] Reloading redacted config from configerator (glob)
  A=c1c5eb4a15a4c71edae31c84f8b23ec5008ad16be07fba5b872fe010184b16ba
  B=749add4e33cf83fda6cce6f4fb4e3037a171dd8068acef09b336fd8ae027bf6f
  C=93cd0903625ea3162047e2699c2ea20d531b634df84180dbeeeb4b62f8afa8cd

  $ mononoke_newadmin fetch -R repo -i 93cd0903625ea3162047e2699c2ea20d531b634df84180dbeeeb4b62f8afa8cd
  *] Reloading redacted config from configerator (glob)
  BonsaiChangesetId: 93cd0903625ea3162047e2699c2ea20d531b634df84180dbeeeb4b62f8afa8cd
  Author: author
  Message: C
  FileChanges:
  	 ADDED/MODIFIED: C 896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  
  $ mononoke_newadmin fetch -R repo -i c1c5eb4a15a4c71edae31c84f8b23ec5008ad16be07fba5b872fe010184b16ba --json | jq -S .
  *] Reloading redacted config from configerator (glob)
  {
    "author": "author",
    "author_date": "1970-01-01T00:00:00+00:00",
    "changeset_id": "c1c5eb4a15a4c71edae31c84f8b23ec5008ad16be07fba5b872fe010184b16ba",
    "committer": null,
    "committer_date": null,
    "extra": {
      "example_extra": [
        49,
        50,
        51,
        255
      ]
    },
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
