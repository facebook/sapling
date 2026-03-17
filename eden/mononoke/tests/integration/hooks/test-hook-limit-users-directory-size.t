# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BYTE_LIMIT=500
  $ ADDITIONAL_DERIVED_DATA="fsnodes" hook_test_setup \
  > limit_users_directory_size <(
  >   cat <<CONF
  > config_json='''{
  >   "directory_size_limit": $BYTE_LIMIT,
  >   "path_prefix": "users"
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

Large file that exceeds the limit at depth-2 -- should be rejected
  $ hg up -q "min(all())"
  $ mkdir -p users/bob/project2
  $ dd if=/dev/zero bs=501 count=1 2>/dev/null | tr '\0' 'x' > users/bob/project2/bigfile.bin
  $ hg ci -Aqm "oversized directory"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_users_directory_size for *: Directory 'users/bob/project2' is 501 bytes (0 MB), which exceeds the size limit of 500 bytes (0 MB). Please reduce the size of this directory before pushing. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

Deep nesting under depth-2 dir counts towards its recursive size -- should be rejected
  $ hg up -q "min(all())"
  $ mkdir -p users/carol/repo/src/deep/nested
  $ dd if=/dev/zero bs=501 count=1 2>/dev/null | tr '\0' 'y' > users/carol/repo/src/deep/nested/huge.dat
  $ hg ci -Aqm "deeply nested large file"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_users_directory_size for *: Directory 'users/carol/repo' is 501 bytes (0 MB), which exceeds the size limit of 500 bytes (0 MB). Please reduce the size of this directory before pushing. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

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

Incremental growth: first commit under limit, second pushes over -- should be rejected
  $ hg up -q "min(all())"
  $ mkdir -p users/dave/workspace
  $ dd if=/dev/zero bs=499 count=1 2>/dev/null | tr '\0' 'a' > users/dave/workspace/file1.bin
  $ hg ci -Aqm "just under limit"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

  $ echo "extra data" > users/dave/workspace/file2.txt
  $ hg ci -Aqm "push over limit"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_users_directory_size for *: Directory 'users/dave/workspace' is * bytes (* MB), which exceeds the size limit of 500 bytes (0 MB). Please reduce the size of this directory before pushing. (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

Two depth-2 dirs each under limit but combined over -- should be accepted (independent limits)
  $ hg up -q "min(all())"
  $ mkdir -p users/eve/proj1 users/eve/proj2
  $ dd if=/dev/zero bs=300 count=1 2>/dev/null | tr '\0' 'e' > users/eve/proj1/data.bin
  $ dd if=/dev/zero bs=300 count=1 2>/dev/null | tr '\0' 'f' > users/eve/proj2/data.bin
  $ hg ci -Aqm "two dirs each under limit"
  $ hg push -r . --to master_bookmark
  pushing rev * to destination mono:repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
