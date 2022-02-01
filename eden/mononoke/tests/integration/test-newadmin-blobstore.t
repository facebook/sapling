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

Check we can upload and fetch an arbitrary blob.
  $ echo value > "$TESTTMP/value"
  $ mononoke_newadmin blobstore -R repo upload --key somekey --value-file "$TESTTMP/value"
  Writing 6 bytes to blobstore key somekey
  $ mononoke_newadmin blobstore -R repo fetch -q somekey -o "$TESTTMP/fetched_value"
  $ diff "$TESTTMP/value" "$TESTTMP/fetched_value"

Test we can unlink a blob

NOTE: The blobstore-unlink command currently only works for sqlblob, and
doesn't construct the blobstore in the usual way, so we need to give the full
key.

  $ mononoke_newadmin blobstore-unlink -R repo repo0000.somekey
  Unlinking key repo0000.somekey
  $ mononoke_newadmin blobstore -R repo fetch -q somekey -o "$TESTTMP/fetched_value_unlinked"
  No blob exists for somekey

Examine some of the data
  $ mononoke_newadmin blobstore -R repo fetch changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  Key: changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec
  Ctime: * (glob)
  Size: 69
  
  BonsaiChangeset {
      inner: BonsaiChangesetMut {
          parents: [],
          author: "test",
          author_date: DateTime(
              1970-01-01T00:00:00+00:00,
          ),
          committer: None,
          committer_date: None,
          message: "A",
          extra: {},
          file_changes: {
              MPath("A"): Change(
                  TrackedFileChange {
                      inner: BasicFileChange {
                          content_id: ContentId(
                              Blake2(eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9),
                          ),
                          file_type: Regular,
                          size: 1,
                      },
                      copy_from: None,
                  },
              ),
          },
          is_snapshot: false,
      },
      id: ChangesetId(
          Blake2(9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec),
      ),
  }

  $ mononoke_newadmin blobstore --storage-name blobstore fetch repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  Key: repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  Ctime: * (glob)
  Size: 4
  
  00000000: 41                                A
