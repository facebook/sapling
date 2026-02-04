---
phase: 12-merge-ci-status
plan: 01
subsystem: api
tags: [github, graphql, typescript, ci-status, mergeability]

# Dependency graph
requires:
  - phase: 11-review-submission
    provides: GitHubDiffSummary type and PR data fetching
provides:
  - Extended GitHubDiffSummary with mergeability and detailed CI check data
  - CICheckRun type for individual CI check status
  - GraphQL queries fetch mergeable, mergeStateStatus, viewerCanMergeAsAdmin
  - extractCIChecks helper for parsing GitHub Checks API and legacy status API
affects: [12-02, 12-03, 12-04]

# Tech tracking
tech-stack:
  added: []
  patterns: [extractCIChecks helper for CI data parsing, GraphQL context extraction for nested check data]

key-files:
  created: []
  modified:
    - addons/isl/src/types.ts
    - addons/isl-server/src/github/githubCodeReviewProvider.ts
    - addons/isl-server/src/github/queries/YourPullRequestsQuery.graphql
    - addons/isl-server/src/github/queries/YourPullRequestsWithoutMergeQueueQuery.graphql

key-decisions:
  - "Use any type for extractCIChecks parameter due to complex generated GraphQL types"
  - "Extract both CheckRun (GitHub Checks API) and StatusContext (legacy status API) for broad CI support"
  - "Map legacy StatusContext state to CheckRun conclusion for unified format"

patterns-established:
  - "extractCIChecks helper pattern: parse nested GraphQL contexts, handle both __typename variants"
  - "New PR fields added to both YourPullRequestsQuery and YourPullRequestsWithoutMergeQueueQuery for consistency"

# Metrics
duration: 4min
completed: 2026-02-02
---

# Phase 12 Plan 01: GitHub PR Data Layer Enhancement Summary

**Extended PR data with mergeability state, merge status, detailed CI check runs, and admin merge capability**

## Performance

- **Duration:** 4 min
- **Started:** 2026-02-02T21:08:26Z
- **Completed:** 2026-02-02T21:12:15Z
- **Tasks:** 2
- **Files modified:** 5 (2 source files, 2 GraphQL queries, 1 generated)

## Accomplishments
- Added CICheckRun, MergeableState, MergeStateStatus types to types.ts
- Extended GitHubDiffSummary with mergeable, mergeStateStatus, ciChecks, viewerCanMergeAsAdmin fields
- Updated GraphQL queries to fetch detailed CI check contexts (CheckRun + StatusContext)
- Implemented extractCIChecks helper to parse nested GraphQL data
- Regenerated TypeScript types from updated GraphQL queries

## Task Commits

Each task was committed atomically:

1. **Task 1: Add CICheckRun type and extend GitHubDiffSummary** - `7a1123eb8b` (feat)
2. **Task 2: Update GraphQL query and data extraction** - `4cd7fe5528` (feat)

## Files Created/Modified
- `addons/isl/src/types.ts` - Added CICheckRun, MergeableState, MergeStateStatus types
- `addons/isl-server/src/github/githubCodeReviewProvider.ts` - Extended GitHubDiffSummary type, added extractCIChecks helper, mapped new fields in PR data extraction
- `addons/isl-server/src/github/queries/YourPullRequestsQuery.graphql` - Added mergeable, mergeStateStatus, viewerCanMergeAsAdmin, and statusCheckRollup.contexts fields
- `addons/isl-server/src/github/queries/YourPullRequestsWithoutMergeQueueQuery.graphql` - Same fields added for merge queue-less query
- `addons/isl-server/src/github/generated/graphql.ts` - Regenerated TypeScript types via codegen

## Decisions Made

1. **Use any type for extractCIChecks parameter**: The generated GraphQL types are deeply nested with complex nullable unions. Using `any` avoids type compatibility issues while maintaining type safety on the return value.

2. **Extract both CheckRun and StatusContext**: GitHub supports two CI APIs - the modern Checks API (CheckRun) and legacy status API (StatusContext). Supporting both ensures compatibility with all CI systems.

3. **Map legacy StatusContext to CheckRun format**: StatusContext uses different field names (context, state, targetUrl) vs CheckRun (name, status/conclusion, detailsUrl). The helper normalizes both to CICheckRun format for consistent UI consumption.

4. **Optional fields with undefined fallback**: All new fields are optional on GitHubDiffSummary since older GitHub instances or PRs without CI may not have these fields populated.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None - GraphQL codegen regenerated successfully, types compiled without errors.

## Next Phase Readiness

**Ready for 12-02 (Merge Button UI):**
- PR data now includes mergeable state (MERGEABLE, CONFLICTING, UNKNOWN)
- PR data includes mergeStateStatus for detailed blocking reasons (BEHIND, BLOCKED, CLEAN, DIRTY, etc.)
- PR data includes viewerCanMergeAsAdmin for admin bypass UI

**Ready for 12-03 (CI Status Display):**
- ciChecks array provides individual check details (name, status, conclusion, detailsUrl)
- Both GitHub Checks API (CheckRun) and legacy status API (StatusContext) supported
- Handles running (IN_PROGRESS/PENDING/QUEUED), completed (SUCCESS/FAILURE), and edge cases (NEUTRAL/SKIPPED/CANCELLED)

No blockers or concerns.

---
*Phase: 12-merge-ci-status*
*Completed: 2026-02-02*
