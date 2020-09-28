# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"


  $ hook_test_setup \
  > limit_path_length <(
  >   cat <<CONF
  > config_strings={length_limit="490"}
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
  $ hgmn push -r . --to master_bookmark
  pushing rev 9af0f6fef03e to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_path_length for 9af0f6fef03e3490dddf78cc54e01e787d8a0046: Path component length for "GQLG:Intern::PlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionResponsePayload::EntPlatformToolViewerContextCallsiteMigrationRuleAction::genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionMutationType.php.i" (256) exceeds length limit (>= 255)
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     limit_path_length for 9af0f6fef03e3490dddf78cc54e01e787d8a0046: Path component length for "GQLG:Intern::PlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionResponsePayload::EntPlatformToolViewerContextCallsiteMigrationRuleAction::genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionMutationType.php.i" (256) exceeds length limit (>= 255)
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nlimit_path_length for 9af0f6fef03e3490dddf78cc54e01e787d8a0046: Path component length for \"GQLG:Intern::PlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionResponsePayload::EntPlatformToolViewerContextCallsiteMigrationRuleAction::genPerformGraphQLPlatformToolViewerContextCallsiteMigrationRuleChangeRuleDescriptionMutationType.php.i\" (256) exceeds length limit (>= 255)"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ hg up -q master_bookmark
  $ mkdir -p "$DIR"
  $ touch "$DIR/$NOT_TOO_LARGE_FILE"
  $ hg ci -Aqm not_too_large
  $ hgmn push -r . --to master_bookmark
  pushing rev 7dfdeae7524e to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
