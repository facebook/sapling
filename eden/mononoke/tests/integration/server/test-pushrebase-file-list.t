# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export PUSHREBASE_REWRITE_DATES=1

  $ setconfig push.edenapi=true
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Push a directory
  $ hg up -q "min(all())"
  $ mkdir dir
  $ echo 1 > dir/1
  $ echo 2 > dir/2
  $ echo 3 > dir/3
  $ hg -q addremove
  $ hg ci -m 'create dir'
  $ hg push -r . --to master_bookmark -q
  $ hg up master_bookmark -q

Now replace directory with a file and push it. Make sure file lists before push
and after push match
  $ hg rm dir
  removing dir/1
  removing dir/2
  removing dir/3
  $ echo dir > dir
  $ hg addremove -q
  $ hg ci -m 'replace directory with a file'

List of files before the push
  $ hg log -r . -T '{files}'
  dir dir/1 dir/2 dir/3 (no-eol)

  $ hg push -r . --to master_bookmark -q
  $ hg up master_bookmark -q

List of files after the push.
  $ hg log -r . -T '{files}'
  dir dir/1 dir/2 dir/3 (no-eol)
