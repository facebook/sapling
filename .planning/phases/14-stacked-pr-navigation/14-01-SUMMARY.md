---
phase: 14-stacked-pr-navigation
plan: 01
subsystem: ui
tags: [jotai, react, typescript, pr-review, stacks]

# Dependency graph
requires:
  - phase: 09-review-mode-foundation
    provides: reviewModeAtom for tracking current PR being reviewed
  - phase: 08-pr-list-view
    provides: allDiffSummaries with GitHub PR data and stackInfo
provides:
  - currentPRStackContextAtom that exposes stack navigation context
  - StackNavigationContext type for stack navigation state
affects: [14-02, 14-03, 14-04]

# Tech tracking
tech-stack:
  added: []
  patterns: [derived-atoms-for-navigation-state]

key-files:
  created: []
  modified:
    - addons/isl/src/codeReview/PRStacksAtom.ts

key-decisions:
  - "Atom returns null when not in review mode (clean boundary)"
  - "Single PR case returns isSinglePr: true with single-entry array (consistent structure)"
  - "Missing PRs in stack get placeholder data (graceful degradation)"

patterns-established:
  - "Derived atoms for navigation context (compute from existing state, no new storage)"

# Metrics
duration: 2min
completed: 2026-02-02
---

# Phase 14 Plan 01: Stack Navigation Context Atom

**Jotai-based stack navigation context atom that derives PR stack relationships from review mode and diff summaries**

## Performance

- **Duration:** 2 min
- **Started:** 2026-02-02T23:48:47Z
- **Completed:** 2026-02-02T23:50:19Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Created `StackNavigationContext` type with stack position and entry details
- Implemented `currentPRStackContextAtom` that derives context from review mode state
- Atom returns null when not in review mode for clean boundaries
- Handles both single PRs and stacked PRs with consistent structure
- Includes full PR details (headHash, title, state) for each stack entry

## Task Commits

Each task was committed atomically:

1. **Task 1: Add StackNavigationContext type and currentPRStackContextAtom** - `a90518cc60` (feat)

## Files Created/Modified
- `addons/isl/src/codeReview/PRStacksAtom.ts` - Added StackNavigationContext type and currentPRStackContextAtom that derives stack navigation context from reviewModeAtom and allDiffSummaries

## Decisions Made
- **Null return when not in review mode:** Clean boundary - only provide context when actually reviewing
- **isSinglePr flag with single-entry array:** Consistent data structure for both single and stacked PRs simplifies consuming components
- **Placeholder data for missing PRs:** If a PR in stackInfo isn't in allDiffSummaries, include it with minimal data (PR number and generic title) rather than skipping it - graceful degradation for loading states

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None - straightforward implementation using existing getStackInfo helper and atom patterns.

## Next Phase Readiness

Ready for 14-02 (Stack Navigation UI). The atom provides all necessary context:
- Current position in stack (currentIndex)
- Total stack size
- Full entry details for each PR (number, hash, title, state)
- isSinglePr flag to conditionally show/hide navigation UI

---
*Phase: 14-stacked-pr-navigation*
*Completed: 2026-02-02*
