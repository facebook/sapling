# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BYTE_LIMIT=500
  $ ADDITIONAL_DERIVED_DATA="content_manifests" hook_test_setup \
  > limit_users_directory_size <(
  >   cat <<CONF
  > config_json='''{
  >   "directory_size_limit": $BYTE_LIMIT,
  >   "path_prefixes": ["users"]
  > }'''
  > CONF
  > )

Small files under depth-2 directory -- should be accepted
  $ hg up -q "min(all())"
  $ mkdir -p users/alice/project1
  $ echo "small content" > users/alice/project1/file1.txt
  $ echo "tiny" > users/alice/project1/file2.txt
  $ hg ci -Aqm "small files under users/alice/project1"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Large file that exceeds the limit at depth-2 -- accepted (hook is not yet implemented)
  $ hg up -q "min(all())"
  $ mkdir -p users/bob/project2
  $ dd if=/dev/zero bs=501 count=1 2>/dev/null | tr '\0' 'x' > users/bob/project2/bigfile.bin
  $ hg ci -Aqm "oversized directory"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Deep nesting under depth-2 dir -- accepted (hook is not yet implemented)
  $ hg up -q "min(all())"
  $ mkdir -p users/carol/repo/src/deep/nested
  $ dd if=/dev/zero bs=501 count=1 2>/dev/null | tr '\0' 'y' > users/carol/repo/src/deep/nested/huge.dat
  $ hg ci -Aqm "deeply nested large file"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Nonexistent prefix -- should be accepted
  $ hg up -q "min(all())"
  $ mkdir -p other/dir/subdir
  $ dd if=/dev/zero bs=501 count=1 2>/dev/null | tr '\0' 'z' > other/dir/subdir/big.bin
  $ hg ci -Aqm "large file outside prefix"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
