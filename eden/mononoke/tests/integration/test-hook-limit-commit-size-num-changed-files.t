# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  > limit_commit_size <(
  >   cat <<CONF
  > config_json='''{
  >   "changed_files_limit": 5,
  >   "too_many_files_message": "Too many files: \${count} > \${limit}.",
  >   "too_large_message": "Too large: \${size} > \${limit}."
  > }'''
  > CONF
  > )

Small commit, one file changed
  $ hg up -q "min(all())"
  $ echo file > file
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev 4f751d63133d to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark


Large commit, a lot of files changed
  $ for x in $(seq 6); do echo $x > $x; done
  $ hg ci -Aqm 2
  $ hgmn push -r . --to master_bookmark
  pushing rev bb41d2a5d8c3 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_size for bb41d2a5d8c3492f085f4d276927533e79f269ae: Too many files: 6 > 5.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commit_size for bb41d2a5d8c3492f085f4d276927533e79f269ae: Too many files: 6 > 5.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commit_size for bb41d2a5d8c3492f085f4d276927533e79f269ae: Too many files: 6 > 5."
  abort: unexpected EOL, expected netstring digit
  [255]
