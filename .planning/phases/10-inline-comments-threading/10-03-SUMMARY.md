---
phase: 10
plan: 03
status: complete
subsystem: comments
tags: [github-api, graphql, replies, threads, diff-view]
dependency-graph:
  requires: ["10-01"]
  provides: ["threadId fetching", "reply functionality", "ReplyInput component"]
  affects: ["10-04", "10-05", "11-01"]
tech-stack:
  added: []
  patterns: ["serverAPI message pattern", "GraphQL mutation for replies"]
key-files:
  created:
    - addons/isl/src/reviewComments/ReplyInput.tsx
  modified:
    - addons/isl/src/types.ts
    - addons/isl/src/codeReview/DiffComments.tsx
    - addons/isl/src/reviewComments/index.ts
    - addons/isl-server/src/github/githubCodeReviewProvider.ts
    - addons/isl-server/src/CodeReviewProvider.ts
    - addons/isl-server/src/ServerToClientAPI.ts
decisions:
  - key: thread-matching-strategy
    choice: "Match threads by path:line key"
    rationale: "Thread ID not directly on review comments, need to fetch separately via reviewThreads"
  - key: reply-submission
    choice: "Immediate submission via GraphQL mutation"
    rationale: "Replies go to existing threads, no need for batching like new comments"
metrics:
  duration: "~5 minutes"
  completed: "2026-02-02"
---

# Phase 10 Plan 03: Thread Replies Summary

Thread reply functionality for GitHub comment threads in diff view.

**One-liner:** Reply to existing threads via GraphQL mutation with inline ReplyInput UI.

## What Was Built

### 1. DiffComment Type Extension
- Added `threadId` field to `DiffComment` type for GitHub thread node ID
- Ensures `isResolved` field is populated from thread info

### 2. Thread Info Fetching
- Added `fetchThreadInfo` method to fetch thread IDs via `reviewThreads` GraphQL query
- Maps threads by `path:line` key to associate with existing comments
- Gracefully handles fetch failures (returns empty map)

### 3. Reply to Thread Method
- Added `replyToThread` method using `addPullRequestReviewThreadReply` GraphQL mutation
- Implements immediate submission (not batched) for existing threads

### 4. ReplyInput Component
- Similar UI to CommentInput (textarea + buttons)
- Loading and error state handling
- Cmd/Ctrl+Enter to submit, Escape to cancel
- Auto-focus on textarea

### 5. Server-side Integration
- Added `graphqlReply` message handler in ServerToClientAPI
- Added `replyToThread` to CodeReviewProvider interface
- Returns `graphqlReplyResult` with success/error

### 6. DiffComments Reply Button
- Reply button (comment icon) appears in byline for threads with `threadId`
- Click opens inline ReplyInput below the comment
- Success refreshes comments to show new reply
- Cancel hides the input

## Technical Details

### Thread Matching Approach
Since `PullRequestReviewComment` doesn't directly expose `pullRequestReviewThread`, we fetch thread info separately:
```graphql
query PullRequestThreadsQuery($url: URI!) {
  resource(url: $url) {
    ... on PullRequest {
      reviewThreads(first: 100) {
        nodes {
          id
          isResolved
          path
          line
        }
      }
    }
  }
}
```

Then map by `path:line` to associate with comments from the existing query.

### Message Flow
1. User clicks Reply button -> `showReply = true`
2. User types and submits -> `graphqlReply` message sent
3. Server calls `replyToThread` -> GraphQL mutation
4. Success/error returned via `graphqlReplyResult`
5. On success: `onRefresh()` called to reload comments

## Files Changed

| File | Changes |
|------|---------|
| `types.ts` | Added `threadId` to DiffComment, added message types |
| `githubCodeReviewProvider.ts` | Added `fetchThreadInfo`, `replyToThread` methods |
| `CodeReviewProvider.ts` | Added `replyToThread` interface method |
| `ServerToClientAPI.ts` | Added `graphqlReply` handler |
| `ReplyInput.tsx` | New component for thread replies |
| `index.ts` | Export ReplyInput |
| `DiffComments.tsx` | Added Reply button and inline input |

## Commits

1. `bb468172fb` - feat(10-03): add threadId to DiffComment and fetch from GitHub API
2. `8ee6c39a2d` - feat(10-03): add ReplyInput component with serverAPI integration
3. `fd2a8fb301` - feat(10-03): add Reply button to comment threads in DiffComments

## Deviations from Plan

### Thread ID Fetching Strategy
**Found during:** Task 1
**Issue:** The `PullRequestReviewComment` GraphQL type doesn't directly expose `pullRequestReviewThread` in the existing query
**Solution:** Added separate `fetchThreadInfo` query that fetches `reviewThreads` directly, then maps by `path:line` key
**Impact:** Slight performance overhead (extra query), but cleaner than modifying generated GraphQL types

## Verification

- [x] TypeScript compiles for modified files
- [x] DiffComment type has threadId field
- [x] fetchComments populates threadId from GitHub API
- [x] ReplyInput component handles submission via serverAPI
- [x] Server handles graphqlReply and executes GraphQL mutation
- [x] DiffComments shows Reply button on threads
- [x] Reply button opens inline input
- [x] Cancel closes reply input

## Next Phase Readiness

**Ready for 10-04:** Thread resolution functionality
- `threadId` is now available for resolution mutations
- `isResolved` populated for display
- Pattern established for thread-based mutations
