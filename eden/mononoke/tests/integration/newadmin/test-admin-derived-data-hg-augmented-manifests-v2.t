# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

Given a repository with separate publication-off and publication-on histories
and hg_augmented_manifests_v2 enabled
  $ ADDITIONAL_DERIVED_DATA="hg_augmented_manifests_v2" setup_common_config
  $ testtool_drawdag -R repo <<'EOF'
  > A-B-C
  > D-E-F
  > # modify: A "foo/file.txt" "content_a"
  > # modify: B "foo/file.txt" "content_b"
  > # modify: C "bar/file.txt" "content_c"
  > # modify: D "publish/file.txt" "content_d"
  > # modify: E "publish/file.txt" "content_e"
  > # modify: F "publish/other.txt" "content_f"
  > # bookmark: C master
  > # bookmark: F publish_v2
  > EOF
  A=* (glob)
  B=* (glob)
  C=* (glob)
  D=* (glob)
  E=* (glob)
  F=* (glob)

When deriving hg_augmented_manifests_v2 with shared publication disabled
  $ mononoke_admin derived-data -R repo derive -T hg_augmented_manifests_v2 -B master

Then v2 completion exists privately without publishing the shared v1 mapping
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests_v2 -i $A -i $B -i $C
  Derived: * (glob)
  Derived: * (glob)
  Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i $A -i $B -i $C
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)

Then v2 derivation did not derive HgChangesets as a side effect
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i $A -i $B -i $C
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)

When deriving v1 as the shared production baseline for direct verification
  $ mononoke_admin derived-data -R repo derive -T hg_augmented_manifests -B master

Then the shared v1 roots validate against a direct v2 recomputation
  $ mononoke_admin derived-data -R repo verify-aug-direct --bookmark master --last 3 --batch-size 2 --concurrency 1 && echo success || echo failure
  [INFO] verifying up to 3 changesets
  [INFO] progress: batch=1 size=2 processed=2 direct=2 full-v2-fallback=0
  [INFO] progress: batch=2 size=1 processed=3 direct=3 full-v2-fallback=0
  done: processed=3 direct=3 full-v2-fallback=0
  success

Given the second history has no private or shared augmented-manifest roots
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests_v2 -i $D -i $E -i $F
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i $D -i $E -i $F
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)

When enabling shared publication and deriving v2
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:derived_data_pipeline_terminal_stage_prod_mapping": true
  >   }
  > }
  > EOF
  $ mononoke_admin derived-data -R repo derive -T hg_augmented_manifests_v2 -B publish_v2

Then v2 is available and the shared production mapping exists
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests_v2 -i $D -i $E -i $F
  Derived: * (glob)
  Derived: * (glob)
  Derived: * (glob)
  $ mononoke_admin derived-data -R repo exists -T hg_augmented_manifests -i $D -i $E -i $F
  Derived: * (glob)
  Derived: * (glob)
  Derived: * (glob)

Then publication-on v2 derivation still did not derive HgChangesets
  $ mononoke_admin derived-data -R repo exists -T hgchangesets -i $D -i $E -i $F
  Not Derived: * (glob)
  Not Derived: * (glob)
  Not Derived: * (glob)
