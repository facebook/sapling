# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  > limit_filesize <(
  >   cat <<CONF
  > bypass_commit_string="@allow-large-files"
  > config_int_lists={filesize_limits_values=[10]}
  > config_string_lists={filesize_limits_regexes=[".*"]}
  > CONF
  > )

Small file
  $ hg up -q "min(all())"
  $ echo 1 > 1
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev a0c9c5791058 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

Large file
  $ LARGE_CONTENT=11111111111
  $ hg up -q "min(all())"
  $ echo "$LARGE_CONTENT" > largefile
  $ hg ci -Aqm largefile
  $ hgmn push -r . --to master_bookmark
  pushing rev 328ac95dcdf8 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 328ac95dcdf83d6268a174267b666bfefafdfc0b: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for 328ac95dcdf83d6268a174267b666bfefafdfc0b: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for 328ac95dcdf83d6268a174267b666bfefafdfc0b: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes). This limit is enforced for files matching the following regex: \".*\". See https://fburl.com/landing_big_diffs for instructions."
  abort: unexpected EOL, expected netstring digit
  [255]

Bypass
  $ hg commit --amend -m "@allow-large-files"
  $ hgmn push -r . --to master_bookmark
  pushing rev bac6b7a9e627 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
