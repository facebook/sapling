---
phase: 11-review-submission
plan: 01
subsystem: api
tags: [github, graphql, typescript, code-review]

# Dependency graph
requires:
  - phase: 10-inline-comments-threading
    provides: Comment creation and thread management foundation
provides:
  - PR node ID in GitHubDiffSummary for mutation APIs
  - GraphQL query updated to fetch GitHub global node IDs
  - Type-safe nodeId field available for review submission

affects: [11-02, 11-03, 11-04, review-submission]

# Tech tracking
tech-stack:
  added: []
  patterns: [GitHub GraphQL node ID pattern for mutations]

key-files:
  created: []
  modified:
    - addons/isl-server/src/github/queries/YourPullRequestsQuery.graphql
    - addons/isl-server/src/github/generated/graphql.ts
    - addons/isl-server/src/github/githubCodeReviewProvider.ts

key-decisions:
  - "Add nodeId as required field in GitHubDiffSummary for mutation support"
  - "Position nodeId immediately after number field for logical grouping"

patterns-established:
  - "GitHub GraphQL mutations require global node IDs, not PR numbers"

# Metrics
duration: 3min
completed: 2026-02-02
---

# Phase 11 Plan 01: Add PR Node ID Support Summary

**GraphQL query and type system updated with GitHub global node IDs for review submission mutations**

## Performance

- **Duration:** 3 min
- **Started:** 2026-02-02T13:50:20Z
- **Completed:** 2026-02-02T13:53:11Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Added `id` field to YourPullRequestsQuery.graphql for fetching GitHub global node IDs
- Regenerated GraphQL TypeScript types from updated query
- Added `nodeId` field to GitHubDiffSummary type with documentation
- Mapped `summary.id` to `nodeId` in PR summary fetch logic

## Task Commits

Each task was committed atomically:

1. **Task 1: Add id field to YourPullRequestsQuery.graphql** - `3283209` (feat)
2. **Task 2: Regenerate GraphQL types and update GitHubDiffSummary** - `57f2ca6` (feat)

## Files Created/Modified
- `addons/isl-server/src/github/queries/YourPullRequestsQuery.graphql` - Added `id` field to PullRequest fragment for GitHub global node ID
- `addons/isl-server/src/github/generated/graphql.ts` - Regenerated types including `id` field in query response
- `addons/isl-server/src/github/githubCodeReviewProvider.ts` - Added `nodeId: string` to GitHubDiffSummary type and mapped from `summary.id`

## Decisions Made

**Node ID field placement:** Positioned `nodeId` field immediately after `number` field in GitHubDiffSummary type for logical grouping of PR identifiers (number for display, nodeId for mutations).

**Documentation approach:** Added inline comment documenting that nodeId is required for mutations like `addPullRequestReview` to make usage intent clear for future developers.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None - GraphQL codegen worked as expected and TypeScript compilation succeeded without errors.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

Ready for 11-02 (Submit Review UI). The `nodeId` field is now available on all `GitHubDiffSummary` objects and can be used directly in the `addPullRequestReview` mutation.

Key points for next phase:
- `nodeId` is populated from GitHub's GraphQL `id` field (base64-encoded global node ID)
- Access via `diffSummary.nodeId` where `diffSummary` is of type `GitHubDiffSummary`
- Required as `pullRequestId` parameter in GitHub's `addPullRequestReview` mutation

---
*Phase: 11-review-submission*
*Completed: 2026-02-02*
