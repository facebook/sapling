# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ BYTE_LIMIT=10
  $ hook_test_setup \
  > limit_commitsize <(
  >   cat <<CONF
  > bypass_commit_string="@allow-large-files"
  > config_ints={commitsizelimit=${BYTE_LIMIT}}
  > CONF
  > )

Small commit
  $ hg up -q 0
  $ for x in $(seq $BYTE_LIMIT); do echo -n 1 > $x; done
  $ hg ci -Aqm 1
  $ hgmn push -r . --to master_bookmark
  pushing rev e6f2d01a954a to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Large file
  $ LARGE_CONTENT=$(for _ in $(seq $(( $BYTE_LIMIT + 1 ))); do echo -n 1; done)
  $ hg up -q 0
  $ echo -n "$LARGE_CONTENT" > largefile
  $ hg ci -Aqm largefile
  $ hgmn push -r . --to master_bookmark
  pushing rev b4b4dcaa16f9 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commitsize for b4b4dcaa16f97662c6a6e70b6eb8c3af1aea8253: Commit size limit is 10 bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commitsize for b4b4dcaa16f97662c6a6e70b6eb8c3af1aea8253: Commit size limit is 10 bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commitsize for b4b4dcaa16f97662c6a6e70b6eb8c3af1aea8253: Commit size limit is 10 bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions."
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Large commit
  $ hg up -q 0
  $ for x in $(seq $(( $BYTE_LIMIT + 1))); do echo -n 1 > "${x}_b"; done
  $ hg ci -Aqm largecommit
  $ hgmn push -r . --to master_bookmark
  pushing rev 0d437325fdc4 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commitsize for 0d437325fdc4006bbd174b823446331bfa53a68d: Commit size limit is 10 bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_commitsize for 0d437325fdc4006bbd174b823446331bfa53a68d: Commit size limit is 10 bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions.
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_commitsize for 0d437325fdc4006bbd174b823446331bfa53a68d: Commit size limit is 10 bytes. You tried to push commit that is over the limit. See https://fburl.com/landing_big_diffs for instructions."
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Bypass
  $ hg commit --amend -m "@allow-large-files"
  $ hgmn push -r . --to master_bookmark
  pushing rev dcf66a8e39a7 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Removing files whose total size is large should work
  $ hgmn up master_bookmark
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
  $ hgmn push -r . --to master_bookmark
  pushing rev f4021c22aa2d to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
