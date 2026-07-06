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

When verifying all 3 commits by explicit bookmark
Then all selected commits validate successfully
  $ mononoke_admin derived-data -R repo verify-aug-direct --bookmark main --last 3 && echo success || echo failure
  [INFO] verifying 3 changesets
  done: processed=3
  success

When verifying a single commit by end-id
Then the selected commit validates successfully
  $ mononoke_admin derived-data -R repo verify-aug-direct --end-id $B --last 1 && echo success || echo failure
  [INFO] verifying 1 changesets
  done: processed=1
  success
