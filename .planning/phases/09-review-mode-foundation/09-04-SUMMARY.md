---
phase: 09-review-mode-foundation
plan: 04
subsystem: ui
tags: [react, jotai, localStorage, pr-review, file-tracking]

# Dependency graph
requires:
  - phase: 09-02
    provides: reviewedFileKeyForPR function in atoms.ts
  - phase: 09-01
    provides: reviewModeAtom state
provides:
  - PR-aware file review tracking in ComparisonViewFile
  - Auto-reset of viewed status when PR is updated
affects: [10-inline-comments, 11-submit-review]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - PR-aware key generation with useMemo
    - Conditional key format based on review mode

key-files:
  created: []
  modified:
    - addons/isl/src/ComparisonView/ComparisonView.tsx

key-decisions:
  - "Use Number() to convert prNumber string to number for reviewedFileKeyForPR"
  - "useMemo for stable key generation across renders"
  - "Fallback to regular reviewedFileKey when not in review mode"

patterns-established:
  - "PR-aware vs standard key pattern: check reviewMode.active before choosing key function"

# Metrics
duration: 1min
completed: 2026-02-02
---

# Phase 9 Plan 4: PR-Aware File Review Checkmarks Summary

**Wired ComparisonViewFile to use PR-aware keys in review mode for persistent viewed status that resets on PR updates**

## Performance

- **Duration:** 1 min 12 sec
- **Started:** 2026-02-02T13:05:05Z
- **Completed:** 2026-02-02T13:06:17Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- ComparisonViewFile now uses PR-aware key when in review mode
- Viewed file status persists until PR is updated (new commits change headHash)
- Non-review comparisons continue using existing key format unchanged

## Task Commits

Each task was committed atomically:

1. **Task 1: Use PR-aware key in ComparisonViewFile when in review mode** - `41784ef896` (feat)

## Files Created/Modified

- `addons/isl/src/ComparisonView/ComparisonView.tsx` - Added reviewedFileKeyForPR import and useMemo-based key selection

## Decisions Made

- Convert prNumber from string to Number() since reviewedFileKeyForPR expects number
- Use useMemo to ensure stable key reference across renders

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 9 complete: Review mode foundation fully established
- Ready for Phase 10: Inline Comments + Threading
- All REV requirements (REV-01 through REV-04) now implemented

---
*Phase: 09-review-mode-foundation*
*Completed: 2026-02-02*
