# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_sqlite"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

  $ mononoke_newadmin fetch -R repo -i 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  *] Reloading redacted config from configerator (glob)
  BonsaiChangesetId: 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  Author: test
  Message: A
  FileChanges:
  	 ADDED/MODIFIED: A eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  
  $ mononoke_newadmin fetch -R repo -i 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec --json | jq -S .
  *] Reloading redacted config from configerator (glob)
  {
    "author": "test",
    "author_date": "1970-01-01T00:00:00+00:00",
    "changeset_id": "9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec",
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

  $ mononoke_newadmin fetch -R repo -i 9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec -p ""
  *] Reloading redacted config from configerator (glob)
  A 005d992c5dcf32993668f7cede29d296c494a5d9 regular



