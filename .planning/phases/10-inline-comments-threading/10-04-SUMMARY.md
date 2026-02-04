---
phase: 10-inline-comments-threading
plan: 04
subsystem: review-comments
tags: [thread-resolution, graphql, github-api, react-component]

# Dependency graph
requires: [10-03]
provides: [thread-resolution-ui, resolve-thread-api, unresolve-thread-api]
affects: [10-05]

# Tech tracking
tech-stack:
  added: []
  patterns: [optimistic-ui-updates, collapsed-thread-state]

# File tracking
key-files:
  created:
    - addons/isl/src/reviewComments/ThreadResolution.tsx
  modified:
    - addons/isl/src/reviewComments/index.ts
    - addons/isl/src/types.ts
    - addons/isl/src/codeReview/DiffComments.tsx
    - addons/isl-server/src/ServerToClientAPI.ts
    - addons/isl-server/src/github/githubCodeReviewProvider.ts
    - addons/isl-server/src/CodeReviewProvider.ts

# Decisions
decisions:
  - id: collapsed-default
    choice: "Resolved threads start collapsed"
    reason: "Focus on unresolved threads, declutter view"
  - id: optimistic-ui
    choice: "Local state for resolution status"
    reason: "Immediate visual feedback, refresh syncs with server"
  - id: auto-collapse
    choice: "Auto-collapse 500ms after resolution"
    reason: "Smooth UX transition"

# Metrics
metrics:
  duration: ~4 min
  completed: 2026-02-02
---

# Phase 10 Plan 04: Thread Resolution Summary

Thread resolution button and collapsed state for resolved comment threads.

## One-liner

ThreadResolutionButton with GraphQL integration, optimistic UI, and collapsed thread state

## Completed Tasks

| Task | Name | Commit | Key Files |
|------|------|--------|-----------|
| 1 | ThreadResolutionButton with serverAPI integration | adae464 | ThreadResolution.tsx, types.ts, ServerToClientAPI.ts, githubCodeReviewProvider.ts |
| 2 | Add resolution UI to DiffComments | 88651ab | DiffComments.tsx |

## Key Decisions Made

1. **Collapsed default for resolved threads**: Resolved threads start collapsed to focus on outstanding issues. Shows summary preview with author and truncated content.

2. **Optimistic UI with local state**: Resolution state tracked locally in component for immediate feedback. onStatusChange callback allows parent to refresh data from server.

3. **Auto-collapse after resolution**: When user resolves a thread, it auto-collapses after 500ms for smooth transition.

4. **Grey border for resolved threads**: Using `colors.grey` for the resolved thread border since no `subtleBorder` token exists.

## Implementation Details

### ThreadResolutionButton Component

```typescript
export function ThreadResolutionButton({
  threadId,
  isResolved,
  onStatusChange,
}: ThreadResolutionButtonProps) {
  // Loading state during API call
  // Posts resolveThread/unresolveThread message to server
  // Waits for threadResolutionResult response
  // Calls onStatusChange on success
  // Shows toast on error
}
```

### Server-side Integration

Added to `ServerToClientAPI.ts`:
- `resolveThread` message handler
- `unresolveThread` message handler
- Returns `threadResolutionResult` with success/error

Added to `githubCodeReviewProvider.ts`:
- `resolveThread(threadId)` - GraphQL resolveReviewThread mutation
- `unresolveThread(threadId)` - GraphQL unresolveReviewThread mutation

### DiffComments Collapsed State

```tsx
// If resolved and collapsed, show summary
if (isTopLevel && localIsResolved === true && collapsed) {
  return (
    <div onClick={() => setCollapsed(false)}>
      <Icon icon="check" />
      Resolved thread - {author}: {preview}...
    </div>
  );
}
```

## File Changes

### Created
- `addons/isl/src/reviewComments/ThreadResolution.tsx` - Resolution button component

### Modified
- `addons/isl/src/reviewComments/index.ts` - Export ThreadResolutionButton
- `addons/isl/src/types.ts` - Add resolveThread, unresolveThread, threadResolutionResult messages
- `addons/isl/src/codeReview/DiffComments.tsx` - Integration with collapsed state
- `addons/isl-server/src/ServerToClientAPI.ts` - Message handlers
- `addons/isl-server/src/github/githubCodeReviewProvider.ts` - GraphQL mutations
- `addons/isl-server/src/CodeReviewProvider.ts` - Interface methods

## Deviations from Plan

None - plan executed exactly as written.

## Verification Results

- TypeScript compiles: PASS (pre-existing errors unrelated to changes)
- ThreadResolutionButton renders based on isResolved: PASS
- Button posts message to server: PASS
- Server executes GraphQL mutation: PASS
- Collapsed state for resolved threads: PASS
- Expand/collapse toggle works: PASS

## Next Phase Readiness

Ready for 10-05 (comment filtering/search). Thread resolution provides:
- isResolved state available for filtering
- Collapsed threads reduce visual noise
- Foundation for "show only unresolved" filter
