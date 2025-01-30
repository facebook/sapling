# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd "$TESTTMP/mononoke-config"

  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="master_bookmark"
  > CONFIG

  $ register_hook_limit_filesize_global_limit 10 'bypass_commit_string="@allow-large-files"'

  $ cd $TESTTMP

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF


setup repo
  $ testtool_drawdag -R repo <<EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

start mononoke
  $ start_and_wait_for_mononoke_server
Clone the repo
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg push -r . --to master_bookmark -q

Delete a file, make sure that file_size_hook is not called on deleted files
  $ hg up -q tip
  $ hg rm 1
  $ hg ci -m 'delete a file'
  $ hg push -r . --to master_bookmark
  pushing rev 7ebfaf7e72f4 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Send large file
  $ hg up -q "min(all())"
  $ echo 'aaaaaaaaaaa' > largefile
  $ hg ci -Aqm 'largefile'
  $ hg push -r . --to master_bookmark
  pushing rev 55663e031ec9 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 55663e031ec95e5bd19d804f9a09a1dbe4158d2a: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  abort: unexpected EOL, expected netstring digit
  [255]

Bypass large file hook
  $ hg amend -m '@allow-large-files'
  $ hg push -r . --to master_bookmark
  pushing rev 21a9c0feb527 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Send large file inside a directory
  $ hg up -q "min(all())"
  $ mkdir dir/
  $ echo 'aaaaaaaaaaa' > dir/largefile
  $ hg ci -Aqm 'dir/largefile'
  $ hg push -r . --to master_bookmark
  pushing rev b2318b1f5fc8 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for b2318b1f5fc817d36a6d771c5d2d9a0af64dfad0: File size limit is 10 bytes. You tried to push file dir/largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  abort: unexpected EOL, expected netstring digit
  [255]
