# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

Given a repository with three commits and master pointing at the head
  $ setup_common_config
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C
  > # modify: A "foo/file.txt" "content_a"
  > # modify: B "foo/file.txt" "content_b"
  > # modify: C "bar/file.txt" "content_c"
  > # bookmark: C main
  > # bookmark: C master
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

Force old-path derivation so stored data is the oracle (not direct-vs-direct).
  $ merge_just_knobs <<EOF
  > {"bools": {"scm/mononoke:augmented_manifest_direct_derivation": false}}
  > EOF

derive augmented manifests via the old path
  $ mononoke_admin derived-data -R repo derive -T hg_augmented_manifests -B master

When verifying all 3 commits by explicit bookmark in batches of two
Then all selected commits validate successfully and progress is reported per batch
  $ mononoke_admin derived-data -R repo verify-aug-direct --bookmark main --last 3 --batch-size 2 --concurrency 1 && echo success || echo failure
  [INFO] verifying up to 3 changesets
  [INFO] progress: batch=1 size=2 processed=2
  [INFO] progress: batch=2 size=1 processed=3
  done: processed=3
  success

When verifying the first 2 commits using the default master bookmark in single-commit batches
Then the root-side commits validate successfully and the reusable start id is printed
  $ mononoke_admin derived-data -R repo verify-aug-direct --first 2 --batch-size 1 --concurrency 1 && echo success || echo failure
  start-id=* (glob)
  [INFO] verifying up to 2 changesets
  [INFO] progress: batch=1 size=1 processed=1
  [INFO] progress: batch=2 size=1 processed=2
  done: processed=2
  success

When verifying a single commit by end-id
Then the selected commit validates successfully
  $ mononoke_admin derived-data -R repo verify-aug-direct --end-id $B --last 1 && echo success || echo failure
  [INFO] verifying up to 1 changesets
  [INFO] progress: batch=1 size=1 processed=1
  done: processed=1
  success
