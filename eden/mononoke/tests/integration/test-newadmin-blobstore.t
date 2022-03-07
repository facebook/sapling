# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_sqlite"
  $ mononoke_testtool drawdag -R repo <<'EOF'
  > Z-A
  >  \ \
  >   B-C
  > # modify: C file "test content \xaa end"
  > # delete: C Z
  > EOF
  A=e26d4ad219658cadec76d086a28621bc612762d0499ae79ba093c5ec15efe5fc
  B=ecf6ed0f7b5c6d1871a3b7b0bc78b04e2cc036a67f96890f2834b728355e5fc5
  C=f9d662054cf779809fd1a55314f760dc7577eac63f1057162c1b8e56aa0f02a1
  Z=e5c07a6110ea10bbcc576b969f936f91fc0a69df0b9bcf1fdfacbf3add06f07a

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
  $ mononoke_newadmin blobstore -R repo fetch changeset.blake2.f9d662054cf779809fd1a55314f760dc7577eac63f1057162c1b8e56aa0f02a1
  Key: changeset.blake2.f9d662054cf779809fd1a55314f760dc7577eac63f1057162c1b8e56aa0f02a1
  Ctime: * (glob)
  Size: 194
  
  BonsaiChangeset {
      inner: BonsaiChangesetMut {
          parents: [
              ChangesetId(
                  Blake2(e26d4ad219658cadec76d086a28621bc612762d0499ae79ba093c5ec15efe5fc),
              ),
              ChangesetId(
                  Blake2(ecf6ed0f7b5c6d1871a3b7b0bc78b04e2cc036a67f96890f2834b728355e5fc5),
              ),
          ],
          author: "author",
          author_date: DateTime(
              1970-01-01T00:00:00+00:00,
          ),
          committer: None,
          committer_date: None,
          message: "C",
          extra: {},
          file_changes: {
              MPath("C"): Change(
                  TrackedFileChange {
                      inner: BasicFileChange {
                          content_id: ContentId(
                              Blake2(896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d),
                          ),
                          file_type: Regular,
                          size: 1,
                      },
                      copy_from: None,
                  },
              ),
              MPath("Z"): Deletion,
              MPath("file"): Change(
                  TrackedFileChange {
                      inner: BasicFileChange {
                          content_id: ContentId(
                              Blake2(6e07d9ecc025ae219c0ed4dead08757d8962ca7532daf5d89484cadc5aae99d8),
                          ),
                          file_type: Regular,
                          size: 18,
                      },
                      copy_from: None,
                  },
              ),
          },
          is_snapshot: false,
      },
      id: ChangesetId(
          Blake2(f9d662054cf779809fd1a55314f760dc7577eac63f1057162c1b8e56aa0f02a1),
      ),
  }

  $ mononoke_newadmin blobstore --storage-name blobstore fetch repo0000.content.blake2.6e07d9ecc025ae219c0ed4dead08757d8962ca7532daf5d89484cadc5aae99d8
  Key: repo0000.content.blake2.6e07d9ecc025ae219c0ed4dead08757d8962ca7532daf5d89484cadc5aae99d8
  Ctime: * (glob)
  Size: 21
  
  00000000: 7465737420636f6e74656e7420aa2065  test content . e
  00000010: 6e64                              nd

