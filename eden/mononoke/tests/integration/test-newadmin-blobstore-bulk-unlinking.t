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

Prepare the input directory for the bulk unlinking tool
  $ mkdir -p "$TESTTMP/key_inputs"

Prepare the input file that only contains a bad-format key for the bulk unlinking tool
  $ echo some-invliad-key  > "$TESTTMP/key_inputs/bad_format_key_file_0"

Run the bulk unliking tool, we're expecting to see an error saying the key is invalid
  $ mononoke_newadmin blobstore-bulk-unlink --keys-dir "$TESTTMP/key_inputs" --dry-run false --sanitise-regex '^repo[0-9]+\.rawbundle2\..*' --error-log-file "$TESTTMP/unlink_log/log_file"
  Processing keys in file (with dry-run=false): $TESTTMP/key_inputs/bad_format_key_file_0
  Progress: 100.000%	processing took * (glob)
  $ cat "$TESTTMP/unlink_log/log_file"
  {"key":"some-invliad-key","message":"Skip key because it is invalid."}
Clean up the test files
  $ rm -rf "$TESTTMP/key_inputs/*"
  $ rm -rf "$TESTTMP/unlink_log/*"

Prepare the input that is in a correct format, but doesn't match the regex
  $ echo repo0000.content.blake2.6e07d9ecc025ae219c0ed4dead08757d8962ca7532daf5d89484cadc5aae99d8 > "$TESTTMP/key_inputs/bad_format_key_file_0"

Run the bulk unliking tool, we're expecting to see program stop
  $ mononoke_newadmin blobstore-bulk-unlink --keys-dir "$TESTTMP/key_inputs" --dry-run false --sanitise-regex '^repo[0-9]+\.rawbundle2\..*' --error-log-file "$TESTTMP/unlink_log/log_file"
  Processing keys in file (with dry-run=false): $TESTTMP/key_inputs/bad_format_key_file_0
  Error: Key repo0000.content.blake2.6e07d9ecc025ae219c0ed4dead08757d8962ca7532daf5d89484cadc5aae99d8 does not match the sanitise checking regex ^repo[0-9]+\.rawbundle2\..*
  [1]
