# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

Given a repository with three commits and hg_augmented_manifests_v2 enabled
  $ ADDITIONAL_DERIVED_DATA="hg_augmented_manifests_v2" setup_common_config
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

When deriving hg_augmented_manifests_v2 by bookmark
  $ mononoke_admin derived-data -R repo derive -T hg_augmented_manifests_v2 -B master

Then v2 roots exist and are visible through the shared v1 augmented-manifest namespace
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests_v2 -i $A -i $B -i $C
  Derived: * (glob)
  Derived: * (glob)
  Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i $A -i $B -i $C
  Derived: * (glob)
  Derived: * (glob)
  Derived: * (glob)

Then v2 derivation did not derive HgChangesets as a side effect
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i $A -i $B -i $C
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)

When deriving HgChangesets only as the verification oracle
  $ mononoke_admin derived-data -R repo derive -T hgchangesets -B master

Then the v2-written augmented manifests validate against the direct verifier
  $ mononoke_admin derived-data -R repo verify-aug-direct --bookmark master --last 3 --batch-size 2 --concurrency 1 && echo success || echo failure
  [INFO] verifying up to 3 changesets
  [INFO] progress: batch=1 size=2 processed=2 direct=2 full-v2-fallback=0
  [INFO] progress: batch=2 size=1 processed=3 direct=3 full-v2-fallback=0
  done: processed=3 direct=3 full-v2-fallback=0
  success
