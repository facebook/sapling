# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675



Setup the right configuration
  $ merge_just_knobs <<EOF
  > {
  >    "ints": {
  >      "scm/mononoke:zstd_compression_level": 3
  >    }
  > }
  > EOF

start mononoke
  $ start_and_wait_for_mononoke_server

push 1MB file, which is larger then zstd stream buffer size
  $ hg clone -q mono:repo repo_large_file
  $ cd repo_large_file
  $ printf '=%.0s' {1..1048576} > largefile
  $ hg add largefile
  $ hg ci -ma
  $ hg push -r . --to master_bookmark -q


clone and checkout the repository with compression enabled
  $ hg clone -U --debug mono:repo --config mononokepeer.compression=true 2>&1 | grep zstd
  zstd compression on the wire is enabled
  $ cd repo
  $ hg checkout master_bookmark --config mononokepeer.compression=true
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

without compression again, no zstd indicator that compression is used
  $ hg pull --debug 2>&1 | grep -P "(zstd|pulling|checking)"
  pulling from mono:* (glob)
