---
phase: 09-review-mode-foundation
plan: 02
subsystem: ui
tags: [react, jotai, localStorage, pr-review, file-tracking]

# Dependency graph
requires:
  - phase: 09-01
    provides: "PR-aware review state selector atom"
provides:
  - "PR-aware reviewed file key generation function"
  - "Key format: pr:{prNumber}:{headHash}:{filePath}"
  - "Automatic viewed status reset on PR updates"
affects: [09-03, 09-04, 10-review-progress-panel]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "PR key generation with hash for invalidation"

key-files:
  created: []
  modified:
    - addons/isl/src/ComparisonView/atoms.ts

key-decisions:
  - "Key format includes headHash to auto-invalidate on PR updates"
  - "pr: prefix distinguishes PR reviews from regular comparisons"
  - "Existing reviewedFileKey() unchanged for backward compatibility"

patterns-established:
  - "PR file keys: pr:{prNumber}:{headHash}:{filePath}"
  - "Old entries orphaned on PR update, cleaned by 14-day expiry"

# Metrics
duration: 2min
completed: 2026-02-02
---

# Phase 9 Plan 02: PR-Aware Viewed File Key Summary

**PR-specific reviewed file key function with head hash for automatic viewed status reset on PR updates**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-02T00:00:00Z
- **Completed:** 2026-02-02T00:02:00Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments

- Added `reviewedFileKeyForPR()` function for PR-specific file key generation
- Key format `pr:{prNumber}:{headHash}:{filePath}` ensures viewed status resets on PR updates
- Existing `reviewedFileKey()` function unchanged for backward compatibility

## Task Commits

Each task was committed atomically:

1. **Task 1: Add PR-aware reviewed file key function** - `244a70ca37` (feat)

## Files Created/Modified

- `addons/isl/src/ComparisonView/atoms.ts` - Added `reviewedFileKeyForPR()` function

## Decisions Made

- **Key format includes headHash:** When PR receives new commits, the headHash changes, causing localStorage entries with old headHash to be orphaned. This automatically resets viewed status without explicit cleanup.
- **pr: prefix:** Distinguishes PR reviews from regular comparison file keys, preventing collisions.
- **Backward compatibility:** Existing `reviewedFileKey()` function unchanged to maintain non-PR comparison functionality.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `reviewedFileKeyForPR()` ready for use in plan 09-03 and 09-04
- Foundation for per-file checkbox state management in place
- Key generation aligns with existing `reviewedFilesAtom` localStorage pattern

---
*Phase: 09-review-mode-foundation*
*Completed: 2026-02-02*
