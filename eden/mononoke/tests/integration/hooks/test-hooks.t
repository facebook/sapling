# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_mononoke_config
  $ cd "$TESTTMP/mononoke-config"

  $ cat >> repos/repo/server.toml <<CONFIG
  > [[bookmarks]]
  > name="master_bookmark"
  > CONFIG

  $ register_hook_limit_filesize_global_limit 10 'bypass_commit_string="@allow-large-files"'

  $ setup_common_hg_configs
  $ cd $TESTTMP


setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
Clone the repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark -q

Delete a file, make sure that file_size_hook is not called on deleted files
  $ hgmn up -q tip
  $ hg rm 1
  $ hg ci -m 'delete a file'
  $ hgmn push -r . --to master_bookmark
  pushing rev 8ecfb5e6aa64 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Send large file
  $ hg up -q "min(all())"
  $ echo 'aaaaaaaaaaa' > largefile
  $ hg ci -Aqm 'largefile'
  $ hgmn push -r . --to master_bookmark
  pushing rev 3e0db158edcc to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 3e0db158edcc82d93b971f44c13ac74836db5714: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for 3e0db158edcc82d93b971f44c13ac74836db5714: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for 3e0db158edcc82d93b971f44c13ac74836db5714: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions."
  abort: unexpected EOL, expected netstring digit
  [255]

Bypass large file hook
  $ hg amend -m '@allow-large-files'
  $ hgmn push -r . --to master_bookmark
  pushing rev ba007efeea59 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev cbc62a724366 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for cbc62a724366fbea4663ca3e1f1a834af9f2f992: File size limit is 10 bytes. You tried to push file dir/largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for cbc62a724366fbea4663ca3e1f1a834af9f2f992: File size limit is 10 bytes. You tried to push file dir/largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for cbc62a724366fbea4663ca3e1f1a834af9f2f992: File size limit is 10 bytes. You tried to push file dir/largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions."
  abort: unexpected EOL, expected netstring digit
  [255]
