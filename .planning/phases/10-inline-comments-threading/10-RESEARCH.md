# Phase 10: Inline Comments + Threading - Research

**Researched:** 2026-02-02
**Domain:** GitHub PR Comments, Diff Annotation, Review Workflow
**Confidence:** HIGH

## Summary

This phase adds inline comment capabilities to the review mode established in Phase 9. The primary challenge is implementing GitHub's pending review workflow where comments are batched before submission. The research confirms that GitHub's GraphQL API supports batch review submission via `addPullRequestReview` with `threads` array, but does NOT support incremental pending reviews (you cannot add comments to an existing pending review). This means all pending comments must be managed client-side and submitted together.

ISL already has extensive comment display infrastructure (`DiffComment` type, `DiffComments.tsx`, `InlineComment.tsx`) that can be extended for authoring. The diff rendering in `SplitDiffView` includes line number cells with `data-line-number`, `data-path`, and `data-side` attributes - perfect hook points for comment affordances.

**Primary recommendation:** Use Jotai `atomFamily` for pending comment state keyed by PR number, implement batch submission via `addPullRequestReview` GraphQL mutation, and extend `SplitDiffRow` with click handlers for comment creation.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Jotai | existing | State management for pending comments | Already used throughout ISL, atomFamily pattern proven |
| GitHub GraphQL | v4 | Comment creation, thread resolution | Only API that supports batch review with threads |
| gh CLI | existing | Execute GraphQL mutations | Already used by `queryGraphQL.ts`, handles auth |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| @stylexjs/stylex | existing | Comment UI styling | Consistent with existing comment components |
| isl-components | existing | Button, Tooltip, TextField | Standard ISL component library |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| GraphQL batch | Individual REST calls | REST doesn't support pending reviews at all |
| atomFamily | useState | Would lose state on component unmount during navigation |
| Client-side pending | Server pending | GitHub API doesn't support incremental pending reviews |

**Installation:**
```bash
# No new dependencies needed - all infrastructure exists
```

## Architecture Patterns

### Recommended Project Structure
```
addons/isl/src/
  reviewComments/           # New directory for review comment functionality
    pendingCommentsState.ts # Jotai atoms for pending comments
    CommentInput.tsx        # Inline comment editor component
    ReviewSubmissionModal.tsx # Modal to confirm/edit before submission
    index.ts                # Exports
  ComparisonView/
    SplitDiffView/
      SplitDiffRow.tsx      # Add comment affordance click handlers
  codeReview/
    DiffComments.tsx        # Extend for reply functionality
```

### Pattern 1: Pending Comments State with atomFamily
**What:** Store pending comments per-PR using Jotai atomFamily with localStorage persistence
**When to use:** Any time we need PR-scoped state that persists across navigation
**Example:**
```typescript
// Source: Pattern from addons/isl/src/ComparisonView/atoms.ts
import { atomFamilyWeak, localStorageBackedAtomFamily } from '../jotaiUtils';
import { atom } from 'jotai';

// Type for a pending comment (not yet submitted)
export type PendingComment = {
  id: string;                     // Unique client-side ID
  type: 'inline' | 'file' | 'pr'; // COM-01, COM-02, COM-03
  body: string;
  path?: string;                  // For inline/file comments
  line?: number;                  // For inline comments
  side?: 'LEFT' | 'RIGHT';        // Which side of diff
  startLine?: number;             // For multi-line comments
  replyToThreadId?: string;       // For replies to existing threads
};

// Per-PR pending comments state
export const pendingCommentsAtom = atomFamilyWeak(
  (prNumber: string) =>
    atom<PendingComment[]>([])
);

// Optional: localStorage persistence for crash recovery
export const pendingCommentsPersistedAtom = localStorageBackedAtomFamily<
  string, // PR number
  PendingComment[]
>(
  'isl.pending-comments:',
  () => [],
  7 // Expire after 7 days
);
```

### Pattern 2: Click Handler on Diff Line Numbers
**What:** Attach click handlers to line number cells to open comment input
**When to use:** For inline comment creation (COM-01)
**Example:**
```typescript
// Source: Pattern from addons/isl/src/ComparisonView/SplitDiffView/SplitDiffRow.tsx
// Existing line number cell has data attributes:
<td
  className={`lineNumber${extraClassName} lineNumber-${side}`}
  data-line-number={lineNumber}
  data-path={path}
  data-side={side}
  data-column={column}
  // Add click handler when in review mode
  onClick={onLineClick ? () => onLineClick(lineNumber, side) : undefined}
>
  {lineNumber}
</td>
```

### Pattern 3: Batch Review Submission via GraphQL
**What:** Submit all pending comments in single addPullRequestReview mutation
**When to use:** When user clicks "Submit Review" (COM-04)
**Example:**
```typescript
// Source: GitHub GraphQL API, DraftPullRequestReviewThread type
// from addons/isl-server/src/github/generated/graphql.ts:6133

const submitReviewMutation = `
  mutation SubmitReview($input: AddPullRequestReviewInput!) {
    addPullRequestReview(input: $input) {
      pullRequestReview {
        id
        state
      }
    }
  }
`;

// Convert pending comments to threads array
const threads: DraftPullRequestReviewThread[] = pendingComments
  .filter(c => c.type === 'inline')
  .map(comment => ({
    body: comment.body,
    line: comment.line!,
    path: comment.path!,
    side: comment.side as DiffSide,
    startLine: comment.startLine,
    startSide: comment.startLine ? comment.side as DiffSide : undefined,
  }));

// Input structure
const input: AddPullRequestReviewInput = {
  pullRequestId: prNodeId,  // Need to fetch PR node ID
  event: PullRequestReviewEvent.Comment, // or Approve, RequestChanges
  body: prLevelComment,     // COM-03 PR-level comment
  threads,                  // Inline comments array
};
```

### Pattern 4: Thread Resolution via GraphQL Mutation
**What:** Resolve/unresolve threads using dedicated mutations
**When to use:** For thread resolution (COM-06)
**Example:**
```typescript
// Source: addons/isl-server/src/github/generated/graphql.ts:22727
// ResolveReviewThreadInput requires thread node ID

const resolveThreadMutation = `
  mutation ResolveThread($input: ResolveReviewThreadInput!) {
    resolveReviewThread(input: $input) {
      thread {
        id
        isResolved
      }
    }
  }
`;

const unresolveThreadMutation = `
  mutation UnresolveThread($input: UnresolveReviewThreadInput!) {
    unresolveReviewThread(input: $input) {
      thread {
        id
        isResolved
      }
    }
  }
`;
```

### Anti-Patterns to Avoid
- **Storing pending comments on server:** GitHub API doesn't support incremental pending reviews - must be client-side
- **Using deprecated addPullRequestReviewComment:** Use addPullRequestReviewThread instead (deprecated 2023-10-01)
- **Submitting comments individually:** Creates notification spam and poor UX; batch with addPullRequestReview
- **Assuming line position === GitHub position:** GitHub uses different position calculation for diffs

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Comment UI | Custom textarea + buttons | Existing `InlineComment.tsx` patterns | Already has styling, author display, reactions |
| Diff position mapping | Manual offset calculation | GitHub's `line` field in current API | New API uses absolute line numbers, not diff positions |
| Thread display | Flat comment list | Existing `DiffComment` with `replies` array | Already handles nested structure |
| Review state | Custom state machine | `reviewModeAtom` from Phase 9 | Already tracks PR being reviewed |
| Comment persistence | Custom localStorage | `localStorageBackedAtomFamily` | Handles TTL, serialization, cleanup |

**Key insight:** The existing comment infrastructure is designed for display; extend it for authoring rather than building parallel components.

## Common Pitfalls

### Pitfall 1: Diff Position vs Line Number Confusion
**What goes wrong:** Old GitHub API used "diff position" (line offset from start of hunk), new API uses actual line numbers
**Why it happens:** Legacy documentation and examples show position-based API
**How to avoid:** Use `line` field (absolute line number), not `position` field; use `side` to indicate LEFT/RIGHT
**Warning signs:** Comments appearing on wrong lines, especially in large diffs

### Pitfall 2: Missing Thread Node IDs for Resolution
**What goes wrong:** Can't resolve threads because we don't have the thread ID from GitHub
**Why it happens:** Current `fetchComments` doesn't return thread IDs, only comment content
**How to avoid:** Extend `PullRequestCommentsQuery` to include `reviewThreads` with `id` and `isResolved`
**Warning signs:** Resolution buttons don't work, or require extra API calls

### Pitfall 3: PR-Level vs Review-Level Comments
**What goes wrong:** "PR-level comments" end up as standalone comments, not part of review
**Why it happens:** GitHub has two types: IssueComments (on PR timeline) and review body comments
**How to avoid:** COM-03 should use `body` field of `addPullRequestReview`, not `addComment` mutation
**Warning signs:** PR-level comments appear separately from review, can't be part of approve/request-changes

### Pitfall 4: Lost Pending Comments on Navigation
**What goes wrong:** User drafts comments, navigates away, comes back to find them gone
**Why it happens:** Atom state cleared on unmount without persistence
**How to avoid:** Use `localStorageBackedAtomFamily` with short TTL (7 days)
**Warning signs:** User complaints about lost work, especially after browser refresh

### Pitfall 5: Stale Comment Position After New Commits
**What goes wrong:** Comment submitted to line that no longer exists in new version
**Why it happens:** PR was updated while user was drafting comments
**How to avoid:** Validate pending comments against current diff before submission; warn user if lines changed
**Warning signs:** API errors on submission, comments on outdated code

## Code Examples

Verified patterns from official sources and existing ISL code:

### Existing DiffComment Type (Extend for Pending)
```typescript
// Source: addons/isl/src/types.ts:151
export type DiffComment = {
  id?: string;
  author: string;
  authorName?: string;
  authorAvatarUri?: string;
  html: string;
  content?: string;
  created: Date;
  commitHash?: string;
  filename?: string;           // For inline comments
  line?: number;               // Line number for inline
  reactions: Array<DiffCommentReaction>;
  suggestedChange?: SuggestedChange;
  replies: Array<DiffComment>;
  isResolved?: boolean;        // Thread resolution state
};
```

### Existing Line Number Cell with Data Attributes
```typescript
// Source: addons/isl/src/ComparisonView/SplitDiffView/SplitDiffRow.tsx:119-128
<td
  className={`lineNumber${extraClassName} lineNumber-${side}`}
  data-line-number={lineNumber}
  data-path={path}
  data-side={side}
  data-column={column}
  onClick={clickableLineNumber ? () => openFileToLine(lineNumber) : undefined}>
  {lineNumber}
</td>
```

### GitHub GraphQL Thread Creation Input
```typescript
// Source: addons/isl-server/src/github/generated/graphql.ts:6133-6146
export type DraftPullRequestReviewThread = {
  /** Body of the comment to leave. */
  body: Scalars['String'];
  /** The line of the blob to which the thread refers. */
  line: Scalars['Int'];
  /** Path to the file being commented on. */
  path: Scalars['String'];
  /** The side of the diff on which the line resides. */
  side?: InputMaybe<DiffSide>;
  /** The first line of the range (for multi-line comments). */
  startLine?: InputMaybe<Scalars['Int']>;
  /** The side of the diff for the start line. */
  startSide?: InputMaybe<DiffSide>;
};
```

### Existing queryGraphQL Pattern
```typescript
// Source: addons/isl-server/src/github/queryGraphQL.ts
// Use this pattern for new mutations
export default async function queryGraphQL<TData, TVariables>(
  query: string,
  variables: TVariables,
  hostname: string,
  timeoutMs?: number,
): Promise<TData>
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `addPullRequestReviewComment` with position | `addPullRequestReviewThread` with line | 2023-10-01 | Must use absolute line numbers, not diff positions |
| Diff-relative positioning | Absolute line + side | 2023-10-01 | Simpler but must know LEFT vs RIGHT |
| Individual comment mutations | Batch via threads array | Current | Better UX, fewer notifications |

**Deprecated/outdated:**
- `position` field in addPullRequestReviewComment: Use `line` in addPullRequestReviewThread
- `commitOID` in comment mutations: Not supported in new API, comments always on HEAD

## Open Questions

Things that couldn't be fully resolved:

1. **Multi-line Comment Support**
   - What we know: `startLine`/`startSide` fields exist for multi-line
   - What's unclear: UI pattern for selecting line range in diff view
   - Recommendation: Start with single-line comments, add multi-line in future phase

2. **PR Node ID Acquisition**
   - What we know: `addPullRequestReview` needs PR node ID, not number
   - What's unclear: Whether current queries return node ID or only number
   - Recommendation: Add `id` field to `YourPullRequestsQuery` response

3. **File-Level Comments (COM-02)**
   - What we know: `subjectType: FILE` option exists in addPullRequestReviewThread
   - What's unclear: Exact API shape for file-level vs line-level
   - Recommendation: Test with `line: null` and `subjectType: FILE`

4. **Reply to Existing Thread**
   - What we know: `addPullRequestReviewThreadReply` mutation exists
   - What's unclear: Whether replies can be pending or must be immediate
   - Recommendation: Implement replies as immediate (not batched), since they're on existing threads

## Sources

### Primary (HIGH confidence)
- `addons/isl-server/src/github/generated/graphql.ts` - Full GitHub GraphQL schema with types
- `addons/isl/src/ComparisonView/SplitDiffView/SplitDiffRow.tsx` - Existing line number rendering
- `addons/isl/src/types.ts` - DiffComment type definition
- `addons/isl/src/jotaiUtils.ts` - atomFamilyWeak, localStorageBackedAtomFamily patterns
- `addons/isl/src/reviewMode.ts` - Phase 9 review mode foundation

### Secondary (MEDIUM confidence)
- [GitHub GraphQL Mutations Docs](https://docs.github.com/en/graphql/reference/mutations) - Official mutation reference
- [GitHub Input Objects Docs](https://docs.github.com/en/graphql/reference/input-objects) - DraftPullRequestReviewThread

### Tertiary (LOW confidence)
- [GitHub CLI Issue #12232](https://github.com/cli/cli/issues/12232) - Community request for review helpers
- [Community Discussion #168380](https://github.com/orgs/community/discussions/168380) - Confirms no incremental pending reviews

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Using existing ISL patterns and official GitHub API
- Architecture: HIGH - Patterns proven in existing codebase
- Pitfalls: MEDIUM - Based on API documentation and community discussions
- GitHub API specifics: HIGH - Verified against generated GraphQL types

**Research date:** 2026-02-02
**Valid until:** 2026-03-02 (30 days - stable GitHub GraphQL API)
