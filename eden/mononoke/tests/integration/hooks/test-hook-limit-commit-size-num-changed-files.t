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
  $ hg push -r . --to master_bookmark
  pushing rev 2102434fc586 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark


Large commit, a lot of files changed
  $ for x in $(seq 6); do echo $x > $x; done
  $ hg ci -Aqm 2
  $ hg push -r . --to master_bookmark
  pushing rev f422eb1076af to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_size for f422eb1076af36a91dcd23d84705f033014bba34: Too many files: 6 > 5.
  abort: unexpected EOL, expected netstring digit
  [255]
