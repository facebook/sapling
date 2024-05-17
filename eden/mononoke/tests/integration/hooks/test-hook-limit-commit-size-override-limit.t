# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BYTE_LIMIT=5
  $ OVERRIDE_BYTE_LIMIT=10
  $ hook_test_setup \
  > limit_commit_size <(
  >   cat <<CONF
  > bypass_commit_string="@allow-large-files"
  > config_json="""{
  >   "commit_size_limit": $BYTE_LIMIT,
  >   "path_overrides": [{"path_regex": "b$", "commit_size_limit": 10}],
  >   "too_many_files_message": "Too many files: \${count} > \${limit}.",
  >   "too_large_message": "Too large: \${size} > \${limit}."
  > }"""
  > CONF
  > )

Large commit
  $ hg up -q "min(all())"
  $ for x in $(seq $(($OVERRIDE_BYTE_LIMIT))); do echo -n 1 > "${x}_b"; done
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev f67c0f33f0f5 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Too large commit
  $ hg up -q "min(all())"
  $ for x in $(seq $(($OVERRIDE_BYTE_LIMIT + 1))); do echo -n 1 > "${x}_b"; done
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev a998ef262b2a to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_size for a998ef262b2a9c8ad130d0fcb11a4577e0ff67a5: Too large: 11 > 10.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commit_size for a998ef262b2a9c8ad130d0fcb11a4577e0ff67a5: Too large: 11 > 10.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commit_size for a998ef262b2a9c8ad130d0fcb11a4577e0ff67a5: Too large: 11 > 10."
  abort: unexpected EOL, expected netstring digit
  [255]
