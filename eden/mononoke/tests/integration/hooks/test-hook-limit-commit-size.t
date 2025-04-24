# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BYTE_LIMIT=10
  $ hook_test_setup \
  > limit_commit_size <(
  >   cat <<CONF
  > bypass_commit_string="@allow-large-files"
  > config_json='''{
  >   "commit_size_limit": $BYTE_LIMIT,
  >   "too_many_files_message": "Too many files: \${count} > \${limit}.",
  >   "too_large_message": "Too large: \${size} > \${limit}."
  > }'''
  > CONF
  > )

Small commit
  $ hg up -q "min(all())"
  $ for x in $(seq $BYTE_LIMIT); do echo -n 1 > $x; done
  $ hg ci -Aqm 1
  $ hg push -r . --to master_bookmark
  pushing rev a86be3e1945e to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Large file
  $ LARGE_CONTENT=$(for _ in $(seq $(( $BYTE_LIMIT + 1 ))); do echo -n 1; done)
  $ hg up -q "min(all())"
  $ echo -n "$LARGE_CONTENT" > largefile
  $ hg ci -Aqm largefile
  $ hg push -r . --to master_bookmark
  pushing rev 1b1ebc46c938 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_size for 1b1ebc46c9382a292384b87c11335846247cbb71: Too large: 11 > 10.
  abort: unexpected EOL, expected netstring digit
  [255]

Large commit
  $ hg up -q "min(all())"
  $ for x in $(seq $(( $BYTE_LIMIT + 1))); do echo -n 1 > "${x}_b"; done
  $ hg ci -Aqm largecommit
  $ hg push -r . --to master_bookmark
  pushing rev ec494c5c2916 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_size for ec494c5c29165309392568089483397dca03d4bf: Too large: 11 > 10.
  abort: unexpected EOL, expected netstring digit
  [255]

Bypass
  $ hg commit --amend -m "@allow-large-files"
  $ hg push -r . --to master_bookmark
  pushing rev 16f05bdad479 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Removing files whose total size is large should work
  $ hg up master_bookmark
  12 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ for x in $(seq $(( $BYTE_LIMIT + 1))); do rm "${x}_b"; done
  $ hg ci -Aqm largeremove
  $ hg status --rev ".^::."
  R 10_b
  R 11_b
  R 1_b
  R 2_b
  R 3_b
  R 4_b
  R 5_b
  R 6_b
  R 7_b
  R 8_b
  R 9_b
  $ hg push -r . --to master_bookmark
  pushing rev 2ba9603286b9 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
