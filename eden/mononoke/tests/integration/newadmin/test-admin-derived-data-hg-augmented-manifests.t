# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

Given a repository with hg_augmented_manifests enabled
  $ setup_common_config
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C
  > # modify: A "foo/file.txt" "content_a"
  > # modify: B "foo/file.txt" "content_b"
  > # modify: C "bar/file.txt" "content_c"
  > # bookmark: C master
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)

When deriving hg_augmented_manifests
  $ mononoke_admin derived-data -R repo derive -T hg_augmented_manifests -B master

Then every root is published
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i $A -i $B -i $C
  Derived: * (glob)
  Derived: * (glob)
  Derived: * (glob)

Then derivation did not derive HgChangesets as a side effect
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i $A -i $B -i $C
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)

Then the published roots validate against a direct recomputation
  $ mononoke_admin derived-data -R repo verify-aug-direct --bookmark master --last 3 --batch-size 2 --concurrency 1 && echo success || echo failure
  [INFO] verifying up to 3 changesets
  [INFO] progress: batch=1 size=2 processed=2 direct=2 full-v2-fallback=0
  [INFO] progress: batch=2 size=1 processed=3 direct=3 full-v2-fallback=0
  done: processed=3 direct=3 full-v2-fallback=0
  success
