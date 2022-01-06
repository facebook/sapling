# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# This test can be seen as a base template for various tests that exercise
# concurrency in the segmented changelog tailer.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-commit.sh"

Setup repository
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'

Seed repository.
  $ quiet segmented_changelog_seeder --head=$A

  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > master_bookmark="master_bookmark"
  > update_algorithm="always_download_save"
  > tailer_update_period_secs=1
  > CONFIG

Run many Segmented Changelog Tailer processes.

  $ background_segmented_changelog_tailer tail_1.out --repo repo
  $ background_segmented_changelog_tailer tail_2.out --repo repo
  $ background_segmented_changelog_tailer tail_3.out --repo repo

  $ hgmn up master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ quiet add_triangle_merge_commits_and_push 1 X
  $ quiet add_triangle_merge_commits_and_push 2 Y
  $ quiet add_triangle_merge_commits_and_push 3 Z


  $ hg log -r "all()" -G -T "{node|short} {desc} {remotenames}" | sed 's/^[ \t]*$/$/'
  @    bbb42bb653ac Z: merge branch 3 default/master_bookmark
  ├─╮
  │ o  d5cb2d73e9d8 Z: commit 5 for branch 3
  │ │
  │ o  f531a8ce7e18 Z: commit 4 for branch 3
  │ │
  │ o  4dddeebd2d4c Z: commit 3 for branch 3
  │ │
  │ o  7caf573a15fa Z: commit 2 for branch 3
  │ │
  │ o  ad915d1dff5b Z: commit 1 for branch 3
  │ │
  o │    b8e3096285c8 Z: merge branch 2
  ├───╮
  │ │ o  d9439ae08482 Z: commit 3 for branch 2
  │ │ │
  │ │ o  730c7f6994e8 Z: commit 2 for branch 2
  │ │ │
  │ │ o  9bffdda333ba Z: commit 1 for branch 2
  │ │ │
  o │ │    3b81a8460440 Z: merge branch 1
  ├─────╮
  │ │ │ o  a7fb21137e80 Z: commit 1 for branch 1
  │ │ │ │
  o │ │ │  8d1d06dcb7c3 Z: base branch 3
  ├─────╯
  o │ │  5d9f6a30ef16 Z: base branch 2
  ├───╯
  o │  65b1a8a22c2e Z: base branch 1
  ├─╯
  o    91b40284f2a9 Y: merge branch 2
  ├─╮
  │ o  cc34d9fe07e5 Y: commit 3 for branch 2
  │ │
  │ o  ce7ddc57f47e Y: commit 2 for branch 2
  │ │
  │ o  8166dac39782 Y: commit 1 for branch 2
  │ │
  o │    50649559e90d Y: merge branch 1
  ├───╮
  │ │ o  a9b7484a7892 Y: commit 1 for branch 1
  │ │ │
  o │ │  9a67d3f3e89e Y: base branch 2
  ├───╯
  o │  94e5292a0790 Y: base branch 1
  ├─╯
  o    5c99bc570462 X: merge branch 1
  ├─╮
  │ o  cd40fe85b470 X: commit 1 for branch 1
  │ │
  o │  49c45f24d44a X: base branch 1
  ├─╯
  o  26805aba1e60 C
  │
  o  112478962961 B
  │
  o  426bada5c675 A
  $

  $ hg log -r "ancestors(master_bookmark)" -T "{node}: {p1node}, {p2node}\n" | sort > "${TESTTMP}/parents.in"

  $ sleep 3

  $ mononoke
  $ wait_for_mononoke

  $ cd ..
  $ hgedenapi debugsegmentclone repo segmentrepo  --traceback
  $ cd segmentrepo

  $ hgedenapi log -r "all()" -G -T "{node|short} {desc} {remotenames}" | sed 's/^[ \t]*$/$/'
  o    bbb42bb653ac Z: merge branch 3 remote/master
  ├─╮
  │ o    b8e3096285c8 Z: merge branch 2
  │ ├─╮
  │ │ o    3b81a8460440 Z: merge branch 1
  │ │ ├─╮
  │ │ │ o  8d1d06dcb7c3 Z: base branch 3
  │ │ │ │
  │ │ o │  a7fb21137e80 Z: commit 1 for branch 1
  │ │ ├─╯
  │ │ o  5d9f6a30ef16 Z: base branch 2
  │ │ │
  │ o │  d9439ae08482 Z: commit 3 for branch 2
  │ │ │
  │ o │  730c7f6994e8 Z: commit 2 for branch 2
  │ │ │
  │ o │  9bffdda333ba Z: commit 1 for branch 2
  │ ├─╯
  │ o  65b1a8a22c2e Z: base branch 1
  │ │
  o │  d5cb2d73e9d8 Z: commit 5 for branch 3
  │ │
  o │  f531a8ce7e18 Z: commit 4 for branch 3
  │ │
  o │  4dddeebd2d4c Z: commit 3 for branch 3
  │ │
  o │  7caf573a15fa Z: commit 2 for branch 3
  │ │
  o │  ad915d1dff5b Z: commit 1 for branch 3
  ├─╯
  o    91b40284f2a9 Y: merge branch 2
  ├─╮
  │ o    50649559e90d Y: merge branch 1
  │ ├─╮
  │ │ o  9a67d3f3e89e Y: base branch 2
  │ │ │
  │ o │  a9b7484a7892 Y: commit 1 for branch 1
  │ ├─╯
  │ o  94e5292a0790 Y: base branch 1
  │ │
  o │  cc34d9fe07e5 Y: commit 3 for branch 2
  │ │
  o │  ce7ddc57f47e Y: commit 2 for branch 2
  │ │
  o │  8166dac39782 Y: commit 1 for branch 2
  ├─╯
  o    5c99bc570462 X: merge branch 1
  ├─╮
  │ o  49c45f24d44a X: base branch 1
  │ │
  o │  cd40fe85b470 X: commit 1 for branch 1
  ├─╯
  o  26805aba1e60 C
  │
  o  112478962961 B
  │
  o  426bada5c675 A
  $

  $ hgedenapi log -r "ancestors('remote/master')" -T "{node}: {p1node}, {p2node}\n" | sort > "${TESTTMP}/parents.out"

  $ diff "${TESTTMP}/parents.in" "${TESTTMP}/parents.out"
