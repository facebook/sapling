---
phase: 12
plan: 03
subsystem: pr-review-merge
tags: [merge, ci, github, operations, state-management]
requires: [12-01, 12-02]
provides:
  - MergePROperation class for executing PR merges
  - mergeability derivation logic (MRG-03)
  - merge state management atoms
affects: [12-04]
decisions:
  - use-run-operation-event: "Use 'RunOperation' TrackEventName (generic event for operations)"
  - merge-via-gh-cli: "Merge via gh CLI using CommandRunner.CodeReviewProvider"
  - non-interactive-mode: "Add --yes flag for non-interactive merge (ISL can't handle prompts)"
  - comprehensive-blocking: "Check CI, reviews, conflicts, branch protection, draft status for mergeability"
tech-stack:
  added: []
  patterns:
    - "Operation subclass for GitHub operations"
    - "Jotai atoms for merge state tracking"
    - "Derivation functions for computed UI state"
key-files:
  created:
    - addons/isl/src/operations/MergePROperation.ts
    - addons/isl/src/reviewMode/mergeState.ts
  modified:
    - addons/isl/src/reviewMode/index.ts
metrics:
  duration: "2m 19s"
  completed: "2026-02-02"
---

# Phase 12 Plan 03: Merge Operation + State Summary

**One-liner:** MergePROperation class with strategy selection and mergeability derivation logic.

## What Was Built

### 1. MergePROperation (operations/MergePROperation.ts)

Created operation class for merging PRs via `gh pr merge`:

- **Strategy selection**: merge, squash, rebase (MRG-02 requirement)
- **Delete branch**: optional --delete-branch flag
- **Non-interactive**: --yes flag for automatic confirmation
- **Uses gh CLI**: CommandRunner.CodeReviewProvider routes to gh instead of sl
- **Display metadata**: Shows strategy label and tooltip with full command

### 2. Merge State Management (reviewMode/mergeState.ts)

Created state atoms and mergeability logic:

- **mergeInProgressAtom**: Tracks active PR merge (prevents double-merge)
- **deriveMergeability**: Implements MRG-03 blocking logic
- **formatMergeBlockReasons**: User-facing display of block reasons

### 3. Mergeability Derivation Logic

Checks for merge-blocking conditions:

1. **PR state**: Already merged/closed
2. **CI status**: Failing or running checks
3. **Reviews**: Changes requested or approval required
4. **Merge conflicts**: CONFLICTING state
5. **Branch protection**: BLOCKED status
6. **Branch sync**: BEHIND base branch
7. **Draft status**: PR marked as draft
8. **Unstable checks**: Required checks not passing

Returns `{canMerge: boolean, reasons: string[]}` for UI display.

## Tasks Completed

| Task | Name                              | Commit  | Files                               |
| ---- | --------------------------------- | ------- | ----------------------------------- |
| 1    | Create MergePROperation           | 975b602 | operations/MergePROperation.ts      |
| 2    | Create merge state + derivation   | 28d033f | reviewMode/mergeState.ts            |
| 3    | Update reviewMode module exports  | 1d8136b | reviewMode/index.ts                 |

## Decisions Made

### 1. Use 'RunOperation' TrackEventName

**Context:** Operation base class requires TrackEventName for analytics.

**Decision:** Use 'RunOperation' (generic event) rather than adding 'MergePROperation' to eventNames.ts.

**Rationale:**
- Keeps eventNames.ts stable (avoid expanding for every operation)
- 'RunOperation' is already used for generic operation tracking
- Operation type is captured in the description field

### 2. Merge via gh CLI (CommandRunner.CodeReviewProvider)

**Context:** Need to execute `gh pr merge` command.

**Decision:** Use CommandRunner.CodeReviewProvider enum value.

**Rationale:**
- Routes to gh CLI rather than sl command
- Consistent with other GitHub operations (PrSubmitOperation pattern)
- Verified enum exists at types.ts:509

### 3. Non-interactive mode (--yes flag)

**Context:** gh CLI prompts for confirmation by default.

**Decision:** Always add --yes flag to gh pr merge command.

**Rationale:**
- ISL can't handle interactive prompts (no stdin)
- User already confirmed via UI button click
- Prevents operation from hanging waiting for input

### 4. Comprehensive blocking checks

**Context:** Need to determine when merge should be disabled (MRG-03).

**Decision:** Check CI, reviews, conflicts, protection rules, draft status, and branch sync.

**Rationale:**
- Matches GitHub's native merge button behavior
- Provides clear reasons for blocking (better UX)
- Prevents merge attempts that would fail server-side

## Technical Notes

### Operation Runner Selection

MergePROperation uses `CommandRunner.CodeReviewProvider`:

```typescript
public runner = CommandRunner.CodeReviewProvider;
```

This routes to gh CLI (not sl). The enum is defined at `types.ts:509`.

### Mergeability State Types

Types imported from existing codebase:

- `DiffSignalSummary`: CI status from Plan 12-01
- `MergeableState`: 'MERGEABLE' | 'CONFLICTING' | 'UNKNOWN' (types.ts:105)
- `MergeStateStatus`: BEHIND, BLOCKED, CLEAN, DIRTY, DRAFT, etc. (types.ts:110)
- `PullRequestReviewDecision`: APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED (GraphQL)

### Reason Priority

Reasons are checked in order:

1. State (merged/closed) - immediate return
2. CI status - critical blocker
3. Reviews - workflow blocker
4. Merge conflicts - technical blocker
5. Branch protection - policy blocker
6. Branch sync - outdated blocker
7. Draft status - workflow blocker
8. Unstable checks - fallback blocker

## Integration Points

### Exports from reviewMode module

```typescript
export {
  mergeInProgressAtom,
  deriveMergeability,
  formatMergeBlockReasons,
} from './mergeState';
export type {MergeabilityStatus, PRMergeabilityData} from './mergeState';
```

### Usage pattern (Plan 12-04)

```typescript
import {deriveMergeability, MergePROperation} from '../reviewMode';

// Derive mergeability
const {canMerge, reasons} = deriveMergeability({
  signalSummary: pr.signalSummary,
  reviewDecision: pr.reviewDecision,
  mergeable: pr.mergeable,
  mergeStateStatus: pr.mergeStateStatus,
  state: pr.state,
});

// Execute merge if allowed
if (canMerge) {
  const op = new MergePROperation(prNumber, strategy, deleteBranch);
  // Run operation...
}
```

## Testing Considerations

### Unit tests needed (future)

1. **deriveMergeability**: Test all blocking conditions
2. **formatMergeBlockReasons**: Test single/multiple reasons
3. **MergePROperation.getArgs**: Verify strategy flags
4. **MergePROperation.getDescriptionForDisplay**: Verify labels

### Manual testing (Plan 12-04)

1. Merge button disabled when CI failing
2. Merge button disabled when reviews pending
3. Merge button shows block reasons on hover
4. Strategy selection updates operation
5. Merge executes and shows progress

## Next Phase Readiness

**Ready for Plan 12-04**: Merge Controls UI

Plan 12-04 can now:
- Use MergePROperation to execute merges
- Use deriveMergeability to enable/disable merge button
- Use formatMergeBlockReasons to show user-facing messages
- Use mergeInProgressAtom to show loading state

**Blockers:** None

**Integration notes:**
- Import from `reviewMode` module (not direct file paths)
- Check canMerge before creating operation
- Set mergeInProgressAtom during operation execution
- Clear mergeInProgressAtom on completion/error

## Deviations from Plan

None - plan executed exactly as written.

## Verification

All verification criteria met:

- ✅ TypeScript compiles without errors (our files)
- ✅ MergePROperation.ts created with strategy support
- ✅ Operation uses CommandRunner.CodeReviewProvider
- ✅ mergeState.ts created with atoms and derivation
- ✅ deriveMergeability implements MRG-03 logic
- ✅ reviewMode/index.ts exports all new items
- ✅ Files exist at expected paths

## Git Activity

**Commits (3):**
```
1d8136b feat(12-03): export merge state utilities from reviewMode module
28d033f feat(12-03): create merge state and mergeability derivation
975b602 feat(12-03): create MergePROperation with strategy selection
```

**Files changed:** 3 created/modified
**Lines added:** ~174
