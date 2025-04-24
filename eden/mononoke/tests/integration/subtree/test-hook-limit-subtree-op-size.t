# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Override subtree key to enable non-test subtree extra
  $ setconfig subtree.use-prod-subtree-key=True
  $ setconfig push.edenapi=true
  $ setconfig subtree.min-path-depth=1
  $ enable amend
  $ setup_common_config
  $ register_hooks \
  > limit_subtree_op_size <(
  >   cat <<CONF
  > bypass_pushvar="TEST_BYPASS=true"
  > config_json='''{
  >   "source_file_count_limit": 2,
  >   "dest_min_path_depth": 2,
  >   "too_many_files_rejection_message": "Too many files in subtree operation copying from \${source_path} to \${dest_path}: \${count} > \${limit}",
  >   "dest_too_shallow_rejection_message": "Subtree operation copying to \${dest_path} has too shallow destination path: < \${dest_min_path_depth}"
  > }'''
  > CONF
  > )

  $ testtool_drawdag -R repo --derive-all --no-default-files << EOF
  > A-B-C-D
  > # modify: A foo/file1 "aaa\n"
  > # modify: A foo/file3 "xxx\n"
  > # copy: B foo/file2 "bbb\n" A foo/file1
  > # delete: B foo/file1
  > # modify: C foo/file2 "ccc\n"
  > # modify: D foo/file4 "yyy\n"
  > # bookmark: D master_bookmark
  > EOF
  A=bad79679db57d8ca7bdcb80d082d1508f33ca2989652922e2e01b55fb3c27f6a
  B=170dbba760afb7ec239d859e2412a827dd7229cdbdfcd549b7138b2451afad37
  C=e611f471e1f2bd488fee752800983cdbfd38d50247e5d81222e0d620fd2a6120
  D=ec1da6035c39c33e159c4baa6b9bfd54676d38a66255ccbcd436f6cfa8ecc2eb

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:repo repo
  $ cd repo

  $ hg update -q master_bookmark^
  $ hg subtree copy -r .^ --from-path foo --to-path bar
  copying foo to bar
  $ ls bar
  file2
  file3
  $ cat bar/file2
  bbb

  $ hg push -q -r . --to master_bookmark
  abort: Server error: hooks failed:
    limit_subtree_op_size for 44bba4bd23a81344c3cbc7f28b42363d4df03399ce6cb0851b51babb48c20549: Subtree operation copying to bar has too shallow destination path: < 2
  [255]

  $ hg update -q master_bookmark
  $ hg subtree copy -r . --from-path foo --to-path bar/baz
  copying foo to bar/baz
  $ hg push -q -r . --to master_bookmark
  abort: Server error: hooks failed:
    limit_subtree_op_size for c5c46cbfe6ecfb38810f51d2d0c3532c38f90d02d483f99d967e14add2be71ff: Too many files in subtree operation copying from foo to bar/baz: 3 > 2
  [255]

  $ hg update -q master_bookmark^
  $ hg subtree copy -r . --from-path foo --to-path bar/baz
  copying foo to bar/baz
  $ hg push -q -r . --to master_bookmark
  $ hg update -q master_bookmark
  $ ls bar/baz
  file2
  file3
