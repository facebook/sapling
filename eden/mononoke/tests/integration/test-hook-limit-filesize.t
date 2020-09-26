# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ hook_test_setup \
  > limit_filesize <(
  >   cat <<CONF
  > bypass_commit_string="@allow-large-files"
  > config_ints={filesizelimit=10}
  > CONF
  > )

Small file
  $ hg up -q 0
  $ echo 1 > 1
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev a0c9c5791058 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Large file
  $ LARGE_CONTENT=11111111111
  $ hg up -q 0
  $ echo "$LARGE_CONTENT" > largefile
  $ hg ci -Aqm largefile
  $ hgmn push -r . --to master_bookmark
  pushing rev 328ac95dcdf8 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 328ac95dcdf83d6268a174267b666bfefafdfc0b: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes).  See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_filesize for 328ac95dcdf83d6268a174267b666bfefafdfc0b: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes).  See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_filesize for 328ac95dcdf83d6268a174267b666bfefafdfc0b: File size limit is 10 bytes. You tried to push file largefile that is over the limit (12 bytes).  See https://fburl.com/landing_big_diffs for instructions."
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Bypass
  $ hg commit --amend -m "@allow-large-files"
  $ hgmn push -r . --to master_bookmark
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
