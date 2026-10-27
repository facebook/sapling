# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Contract: a blob key that any ENFORCED redaction set lists is enforced
# (access denied), whatever other sets list it as log-only and whatever the
# order in which the sets' key lists are fetched; the merged redaction map must
# not depend on fetch order. Two sets share the key of file `c`: a small
# enforced set (T1) and a large log-only set (T0, 1000 more keys, so its
# key-list blob is read last); `c` stays enforced and its entry names both
# sets, whatever the fetch order.

  $ . "${TEST_FIXTURES}/library.sh"

Setup config repo:
  $ setup_common_config blob_files
  $ cd "$TESTTMP"

Commit M is the main bookmark (no redacted content). Commit A, a separate root,
holds file `c` and 1000 other files that only pad the log-only key list:
  $ { echo "M"; echo "A"; echo '# modify: M "m" "m"'; echo '# modify: A "c" "c"'; for i in $(seq 1000); do echo "# modify: A \"pad/f$i\" \"pad $i\""; done; echo "# bookmark: M master_bookmark"; } > "$TESTTMP/dag"
  $ testtool_drawdag -R repo --no-default-files --derive-all < "$TESTTMP/dag"
  A=* (glob)
  M=* (glob)
  $ C_KEY=content.blake2.000a1a9b74aa3da71fcceb653a62cb6987ae440c2b5c3d7e5d08d7c526b1dca8

The content of `c` is readable before any redaction:
  $ mononoke_admin blobstore -R repo fetch "$C_KEY"
  Key: content.blake2.000a1a9b74aa3da71fcceb653a62cb6987ae440c2b5c3d7e5d08d7c526b1dca8
  Ctime: * (glob)
  Size: 4
  
  00000000: 63                                c

Key list 1 (enforced, T1): only `c`. Key list 0 (log-only, T0): `c` and the
1000 padding files:
  $ mononoke_admin redaction create-key-list -R repo -i "$A" c --main-bookmark master_bookmark --output-file rs_enforced --skip-aws-sync
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator
  $ { echo "$A:c"; for i in $(seq 1000); do echo "$A:pad/f$i"; done; } > "$TESTTMP/log_only_pairs"
  $ mononoke_admin redaction create-key-list -R repo --input-file "$TESTTMP/log_only_pairs" --main-bookmark master_bookmark --output-file rs_log_only --skip-aws-sync
  Checking redacted content doesn't exist in 'master_bookmark' bookmark
  No files would be redacted in the main bookmark (master_bookmark)
  Redaction saved as: * (glob)
  To finish the redaction process, you need to commit this id to scm/mononoke/redaction/redaction_sets.cconf in configerator
  $ mononoke_admin redaction fetch-key-list -R repo $(cat rs_enforced) | grep -c "^$C_KEY$"
  1
  $ mononoke_admin redaction fetch-key-list -R repo $(cat rs_log_only) | wc -l
  1001
  $ mononoke_admin redaction fetch-key-list -R repo $(cat rs_log_only) | grep -c "^$C_KEY$"
  1

Both sets are active, the enforced one listed first:
  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T1", "id": "$(cat rs_enforced)", "enforce": true},
  >    {"reason": "T0", "id": "$(cat rs_log_only)", "enforce": false}
  >  ]
  > }
  > EOF

`c` is enforced by T1 although the log-only T0 key list is fetched last, its
entry names both sets, and its content is censored:
  $ mononoke_admin redaction list -R repo -i "$A" | grep ": c"
  T0, T1              : c
  $ mononoke_admin blobstore -R repo fetch "$C_KEY"
  Error: Failed to fetch blob
  
  Caused by:
      The blob content.blake2.000a1a9b74aa3da71fcceb653a62cb6987ae440c2b5c3d7e5d08d7c526b1dca8 is censored.
       Task/Sev: T0, T1
  [1]
