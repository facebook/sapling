# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"


  $ hook_test_setup \
  > limit_path_length <(
  >   cat <<CONF
  > config_json='''{
  >   "length_limit": 490
  > }'''
  > CONF
  > )

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Ok file path - should work
S200449
  $ DIR="flib/intern/__generated__/GraphQLMeerkatStep/flib/intern/entschema/generated/entity/profile_plus/EntPlatformToolViewerContextCallsiteMigrationRuleAction.php"
  $ NOT_TOO_LARGE_FILE="GQLG_Intern__PlatformToolViewerContextCallsiteMigrationRuleChangeRuleApiMappingResponsePayload__EntPlatformToolViewerContextCallsiteMigrationRuleAction__genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleApiMappingMutationType.php"
  $ TOO_LARGE_FILE="GQLG_Intern__PlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionResponsePayload__EntPlatformToolViewerContextCallsiteMigrationRuleAction__genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionMutationType.php"

  $ hg up -q master_bookmark
  $ mkdir -p "$DIR"
  $ touch "$DIR/$TOO_LARGE_FILE"
  $ hg ci -Aqm too_large
  $ hg push -r . --to master_bookmark
  pushing rev a527eef669bc to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_path_length for a527eef669bc53b8c175b62b22a66b84e1c1b6e5: Path component length for "GQLG:Intern::PlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionResponsePayload::EntPlatformToolViewerContextCallsiteMigrationRuleAction::genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionMutationType.php.i" (256) exceeds length limit (>= 255)
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg up -q master_bookmark
  $ mkdir -p "$DIR"
  $ touch "$DIR/$NOT_TOO_LARGE_FILE"
  $ hg ci -Aqm not_too_large
  $ hg push -r . --to master_bookmark
  pushing rev 330a172698db to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
