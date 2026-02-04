# Phase 11: Review Submission - Research

**Researched:** 2026-02-02
**Domain:** GitHub PR Review Submission, Approval Decisions, Review Workflow
**Confidence:** HIGH

## Summary

This phase implements the final step of the review workflow: submitting a complete review with approval decision (APPROVE, REQUEST_CHANGES, or COMMENT) and summary text. The research confirms that GitHub's GraphQL API provides `addPullRequestReview` mutation which creates and submits a review in one operation, accepting a `threads` array for pending comments from Phase 10, an optional `body` for summary text, and a required `event` field for the approval decision.

The key architectural insight is that we need to add the `id` field (global node ID) to `YourPullRequestsQuery.graphql` since `addPullRequestReview` requires `pullRequestId` (node ID), not just the PR number. The GitHub CLI (`gh pr review`) provides commands for all three review types with `--approve`, `--request-changes`, and `--comment` flags, each accepting `--body` for summary text.

UI patterns from existing ISL code show modal-based confirmation patterns (useModal hook with 'custom' type), button placement in ComparisonView, and toast notifications for operation feedback. The submission flow should: 1) show modal with summary textarea and action buttons, 2) convert pending comments to GraphQL threads, 3) submit via GraphQL mutation, 4) clear pending state, 5) show success toast, and 6) exit review mode.

**Primary recommendation:** Add `id` field to YourPullRequestsQuery, use `addPullRequestReview` GraphQL mutation (not `submitPullRequestReview` which is for existing pending reviews), implement modal with useModal hook for summary input and action selection, and leverage Phase 10's pendingCommentsAtom for batch submission.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| GitHub GraphQL API | v4 | Review submission with approval decisions | Only API supporting batch review with approval events |
| gh CLI | existing | Execute GraphQL mutations | Already used by queryGraphQL.ts, handles auth and hostname |
| Jotai | existing | State management for review flow | Proven pattern in ISL, atomFamily for per-PR state |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| useModal hook | existing | Confirmation UI with custom component | Standard pattern for user confirmation in ISL |
| showToast | existing | Success/error feedback | Standard notification pattern |
| @stylexjs/stylex | existing | Component styling | Consistent with ISL styling patterns |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| addPullRequestReview | submitPullRequestReview | submitPullRequestReview requires existing pending review ID, doesn't support creating+submitting in one call |
| GraphQL mutation | gh pr review CLI | CLI is simpler but less flexible; mutation gives better error handling and response data |
| Modal confirmation | Inline form | Modal prevents accidental submission, matches ISL patterns for important actions |

**Installation:**
```bash
# No new dependencies needed - all infrastructure exists
```

## Architecture Patterns

### Recommended Project Structure
```
addons/isl/src/
  reviewComments/
    ReviewSubmissionModal.tsx    # NEW: Modal for summary + action selection
    pendingCommentsState.ts      # Phase 10: Pending comments atom
    index.ts                      # Phase 10: Exports
  ComparisonView/
    ComparisonView.tsx            # Add "Submit Review" button when in review mode
  reviewMode.ts                   # Phase 9: Review mode state

addons/isl-server/src/
  github/
    queries/
      YourPullRequestsQuery.graphql  # MODIFY: Add 'id' field for node ID
    githubCodeReviewProvider.ts      # NEW: submitPullRequestReview handler
  ServerToClientAPI.ts                # NEW: 'submitPullRequestReview' message type
```

### Pattern 1: Add Node ID to PR Query
**What:** Extend YourPullRequestsQuery to fetch PR global node ID
**When to use:** Required for addPullRequestReview mutation
**Example:**
```graphql
# Source: addons/isl-server/src/github/queries/YourPullRequestsQuery.graphql
query YourPullRequestsQuery($searchQuery: String!, $numToFetch: Int!) {
  search(query: $searchQuery, type: ISSUE, first: $numToFetch) {
    nodes {
      ... on PullRequest {
        id          # ADD THIS - global node ID (e.g., "PR_kwDOAHz1OX4uYAah")
        number      # existing - PR number (e.g., 123)
        title
        # ... rest of fields
      }
    }
  }
}
```

### Pattern 2: Review Submission via addPullRequestReview
**What:** Submit review with approval decision and pending comments in single mutation
**When to use:** When user clicks "Submit Review" button (SUB-01, SUB-02, SUB-03)
**Example:**
```typescript
// Source: GitHub GraphQL API, AddPullRequestReviewInput type
// from addons/isl-server/src/github/generated/graphql.ts:446-467

const submitReviewMutation = `
  mutation SubmitReview($input: AddPullRequestReviewInput!) {
    addPullRequestReview(input: $input) {
      pullRequestReview {
        id
        state
        body
      }
    }
  }
`;

// Convert pending comments from Phase 10 to GraphQL threads
const threads: DraftPullRequestReviewThread[] = pendingComments
  .filter(c => c.type === 'inline')
  .map(comment => ({
    body: comment.body,
    line: comment.line!,
    path: comment.path!,
    side: comment.side as DiffSide, // 'LEFT' | 'RIGHT'
    // For multi-line comments (future):
    startLine: comment.startLine,
    startSide: comment.startLine ? (comment.side as DiffSide) : undefined,
  }));

// Input structure
const input: AddPullRequestReviewInput = {
  pullRequestId: prNodeId,  // Global node ID from query (NOT PR number)
  event: PullRequestReviewEvent.Approve, // or RequestChanges, Comment
  body: summaryText,        // Optional summary (SUB-04)
  threads,                  // Inline comments from Phase 10
};

// Execute via queryGraphQL
const result = await queryGraphQL<{
  addPullRequestReview: {
    pullRequestReview: {id: string; state: string; body: string};
  };
}>(submitReviewMutation, {input}, hostname);
```

### Pattern 3: Review Submission Modal UI
**What:** Modal for summary text input and action selection
**When to use:** When user initiates review submission
**Example:**
```typescript
// Source: Pattern from addons/isl/src/useModal.tsx

export function ReviewSubmissionModal({
  returnResultAndDismiss,
}: {
  returnResultAndDismiss: (data: {event: PullRequestReviewEvent; body: string} | null) => void;
}) {
  const [summaryText, setSummaryText] = useState('');

  return (
    <div className="review-submission-modal">
      <h2>Submit Review</h2>
      <textarea
        placeholder="Leave a comment (optional)"
        value={summaryText}
        onChange={e => setSummaryText(e.target.value)}
        rows={5}
      />
      <div className="review-actions">
        <Button
          primary
          icon="check"
          onClick={() => returnResultAndDismiss({
            event: PullRequestReviewEvent.Approve,
            body: summaryText,
          })}>
          Approve
        </Button>
        <Button
          icon="request-changes"
          onClick={() => returnResultAndDismiss({
            event: PullRequestReviewEvent.RequestChanges,
            body: summaryText,
          })}>
          Request Changes
        </Button>
        <Button
          icon="comment"
          onClick={() => returnResultAndDismiss({
            event: PullRequestReviewEvent.Comment,
            body: summaryText,
          })}>
          Comment
        </Button>
        <Button onClick={() => returnResultAndDismiss(null)}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

// Usage with useModal
const showModal = useModal();
const result = await showModal<{event: PullRequestReviewEvent; body: string} | null>({
  type: 'custom',
  component: ReviewSubmissionModal,
  title: 'Submit Review',
  icon: 'send',
});
```

### Pattern 4: Submit Review Button Placement
**What:** Add "Submit Review" button to ComparisonView when in review mode
**When to use:** When reviewMode.active === true and pendingComments.length > 0
**Example:**
```typescript
// Source: Pattern from addons/isl/src/ComparisonView/ComparisonView.tsx

export default function ComparisonView({comparison, dismiss}: Props) {
  const reviewMode = useAtomValue(reviewModeAtom);
  const pendingComments = useAtomValue(
    pendingCommentsAtom(reviewMode.prNumber ?? '')
  );

  // Show submit button in review mode with pending comments
  const showSubmitButton = reviewMode.active && pendingComments.length > 0;

  return (
    <div className="comparison-view">
      {showSubmitButton && (
        <div className="review-submission-bar">
          <Button
            primary
            icon="send"
            onClick={handleSubmitReview}>
            Submit Review ({pendingComments.length} comments)
          </Button>
        </div>
      )}
      {/* ... rest of comparison view ... */}
    </div>
  );
}
```

### Pattern 5: gh CLI Alternative (Simpler Approach)
**What:** Use gh pr review CLI command instead of GraphQL mutation
**When to use:** For simpler implementation without GraphQL complexity
**Example:**
```bash
# Source: https://cli.github.com/manual/gh_pr_review

# Approve with summary
gh pr review 123 --approve --body "LGTM! Great work on the error handling."

# Request changes with summary
gh pr review 123 --request-changes --body "Needs more tests before merge."

# Comment without approval decision
gh pr review 123 --comment --body "Nice refactoring, a few minor suggestions."
```

**Note:** This approach is simpler but doesn't support batch inline comments. Would need separate commands for inline comments (not available in gh CLI as of 2026). Recommendation: Use GraphQL mutation for full feature support.

### Anti-Patterns to Avoid
- **Submitting without pending comments:** User should be able to submit review with just summary text (no inline comments required)
- **Using submitPullRequestReview mutation:** That's for existing pending reviews; we create+submit in one step with addPullRequestReview
- **Forgetting to clear pending state:** After successful submission, must clear pendingCommentsAtom for the PR
- **Not handling partial failures:** If some comments fail but review succeeds, user should know what happened
- **Submitting with PR number instead of node ID:** addPullRequestReview requires global node ID, not PR number

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Modal UI | Custom overlay component | useModal hook with 'custom' type | Handles focus, keyboard shortcuts, accessibility |
| Success notification | Custom notification system | showToast from toast.ts | Consistent with ISL patterns, auto-dismisses |
| GraphQL execution | Custom fetch with auth | queryGraphQL from github/queryGraphQL.ts | Handles gh CLI auth, hostname, timeouts |
| Textarea resizing | Custom resize handlers | Existing textarea patterns in ISL | Consistent styling and behavior |
| Error handling | Custom error display | ErrorNotice component | Standard error UI in ISL |

**Key insight:** ISL has established patterns for all UI interactions needed for review submission. Don't reinvent these patterns.

## Common Pitfalls

### Pitfall 1: Missing PR Node ID
**What goes wrong:** addPullRequestReview mutation fails with "pullRequestId is required"
**Why it happens:** YourPullRequestsQuery currently doesn't fetch `id` field, only `number`
**How to avoid:** Add `id` field to YourPullRequestsQuery.graphql fragment
**Warning signs:** GraphQL errors mentioning "pullRequestId" or "node ID"

### Pitfall 2: Submitting Empty Review
**What goes wrong:** Review submitted with neither comments nor summary text
**Why it happens:** User clicks submit without adding content
**How to avoid:** Validate that either pendingComments.length > 0 OR body.trim().length > 0 before submission
**Warning signs:** GitHub API accepts but review appears empty/useless

### Pitfall 3: Not Clearing Pending State After Success
**What goes wrong:** After submission, pending comments still show in UI
**Why it happens:** Forgot to call clearPendingComments after successful mutation
**How to avoid:** Clear pendingCommentsAtom immediately after successful API response
**Warning signs:** Comments appear to be "pending" after review is live on GitHub

### Pitfall 4: Permission Errors on Protected Branches
**What goes wrong:** User can't approve PR because branch requires review from code owner
**Why it happens:** Protected branch settings override user permissions
**How to avoid:** Check PR reviewDecision field before showing approve button; show tooltip if user lacks permission
**Warning signs:** GraphQL errors mentioning "insufficient permissions" or "branch protection"

### Pitfall 5: Stale Review on PR Update
**What goes wrong:** User submits review, then PR author pushes new commits, old review becomes stale
**Why it happens:** GitHub's "Dismiss stale pull request approvals when new commits are pushed" setting
**How to avoid:** Use PR's headHash in review mode key (already done in Phase 9); when hash changes, pending comments become invalid
**Warning signs:** User confusion when review disappears after PR update

### Pitfall 6: Race Condition with Multiple Tabs
**What goes wrong:** User has ISL open in two tabs, submits review in one, other tab still shows pending comments
**Why it happens:** localStorageBackedAtomFamily doesn't sync across tabs
**How to avoid:** Add storage event listener to sync pending comments across tabs, OR show warning when detecting multi-tab scenario
**Warning signs:** User reports comments "reappearing" after submission

## Code Examples

Verified patterns from official sources and existing ISL code:

### PullRequestReviewEvent Enum (Official GraphQL Types)
```typescript
// Source: addons/isl-server/src/github/generated/graphql.ts:18220-18229

/** The possible events to perform on a pull request review. */
export enum PullRequestReviewEvent {
  /** Submit feedback and approve merging these changes. */
  Approve = 'APPROVE',
  /** Submit general feedback without explicit approval. */
  Comment = 'COMMENT',
  /** Dismiss review so it now longer effects merging. */
  Dismiss = 'DISMISS',
  /** Submit feedback that must be addressed before merging. */
  RequestChanges = 'REQUEST_CHANGES'
}
```

### AddPullRequestReviewInput Type
```typescript
// Source: addons/isl-server/src/github/generated/graphql.ts:446-467

export type AddPullRequestReviewInput = {
  /** The contents of the review body comment. */
  body?: InputMaybe<Scalars['String']>;
  /** A unique identifier for the client performing the mutation. */
  clientMutationId?: InputMaybe<Scalars['String']>;
  /** The commit OID the review pertains to. */
  commitOID?: InputMaybe<Scalars['GitObjectID']>;
  /** The event to perform on the pull request review. */
  event?: InputMaybe<PullRequestReviewEvent>;
  /** The Node ID of the pull request to modify. */
  pullRequestId: Scalars['ID'];
  /** The review line comment threads. */
  threads?: InputMaybe<Array<InputMaybe<DraftPullRequestReviewThread>>>;
};
```

### DraftPullRequestReviewThread Type (from Phase 10 Research)
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

### useModal Pattern for Custom Component
```typescript
// Source: addons/isl/src/useModal.tsx:151-169

export function useModal(): <T>(config: ModalConfig<T>) => Promise<T | undefined> {
  const setModal = useSetAtom(modalState);

  return useCallback(
    <T,>(config: ModalConfig<T>) => {
      const deferred = defer<T | undefined>();
      setModal({
        config: config as ModalConfig<unknown>,
        visible: true,
        deferred: deferred as Deferred<unknown | undefined>,
      });

      return deferred.promise as Promise<T>;
    },
    [setModal],
  );
}
```

### showToast Pattern for Success Feedback
```typescript
// Source: addons/isl/src/toast.ts:28-39

export function showToast(message: ReactNode, props?: {durationMs?: number; key?: string}) {
  const {durationMs = DEFAULT_DURATION_MS, key} = props ?? {};
  writeAtom(toastQueueAtom, oldValue => {
    let nextValue = oldValue;
    const hideAt = new Date(Date.now() + durationMs);
    if (key != null) {
      // Remove an existing toast with the same key.
      nextValue = nextValue.filter(({key: k}) => k !== key);
    }
    return nextValue.push({message, disapparAt: hideAt, key: key ?? hideAt.getTime().toString()});
  });
}
```

### queryGraphQL Pattern for GitHub Mutations
```typescript
// Source: addons/isl-server/src/github/queryGraphQL.ts:12-17

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
| Individual comment mutations | Batch via addPullRequestReview threads | 2023-10-01 | All comments submitted atomically with review |
| submitPullRequestReview for new reviews | addPullRequestReview for create+submit | Current | Simpler one-step flow for typical use case |
| PR number for mutations | Global node ID (base64 encoded) | Current | Must fetch `id` field in queries |
| Pending reviews on server | Client-side pending state | Current | No incremental pending review API support |

**Deprecated/outdated:**
- `submitPullRequestReview` for new reviews: Use `addPullRequestReview` which creates and submits in one call
- `addPullRequestReviewComment` with individual comments: Use `threads` array in `addPullRequestReview`

## Open Questions

Things that couldn't be fully resolved:

1. **Empty Review Submission**
   - What we know: GitHub API allows submitting review with no comments and no body
   - What's unclear: Should ISL allow this, or require at least summary text?
   - Recommendation: Allow empty reviews (user might want to approve without comment), but show confirmation

2. **Review Submission with Only PR-Level Comment**
   - What we know: User can submit review with just `body` text, no inline comments
   - What's unclear: Should this still clear/use pendingCommentsAtom?
   - Recommendation: Yes, user might have drafted inline comments then decided to remove them

3. **Multi-Tab Pending Comment Sync**
   - What we know: localStorageBackedAtomFamily persists but doesn't sync live changes across tabs
   - What's unclear: Should we add storage event listener for cross-tab sync?
   - Recommendation: Start without sync, add if users report issues

4. **Re-Review After PR Update**
   - What we know: When PR headHash changes, pending comments are invalidated by key format
   - What's unclear: Should old pending comments be migrated/preserved somehow?
   - Recommendation: Discard old pending comments, they reference stale code

5. **Approve Without Write Permission**
   - What we know: Protected branches may require specific reviewers
   - What's unclear: How to detect if current user can approve before showing button
   - Recommendation: Show button, handle error gracefully with helpful message

## Sources

### Primary (HIGH confidence)
- `addons/isl-server/src/github/generated/graphql.ts:18220-18229` - PullRequestReviewEvent enum
- `addons/isl-server/src/github/generated/graphql.ts:446-467` - AddPullRequestReviewInput type
- `addons/isl-server/src/github/queries/YourPullRequestsQuery.graphql` - Current PR query structure
- `addons/isl/src/useModal.tsx` - Modal pattern for custom components
- `addons/isl/src/toast.ts` - Toast notification pattern
- `addons/isl/src/reviewMode.ts` - Phase 9 review mode foundation
- `.planning/phases/10-inline-comments-threading/10-RESEARCH.md` - Phase 10 pending comments infrastructure
- [GitHub GraphQL Mutations Docs](https://docs.github.com/en/graphql/reference/mutations) - Official addPullRequestReview reference
- [GitHub GraphQL Enums Docs](https://docs.github.com/en/graphql/reference/enums) - Official PullRequestReviewEvent documentation

### Secondary (MEDIUM confidence)
- [GitHub CLI Manual - gh pr review](https://cli.github.com/manual/gh_pr_review) - CLI commands for review submission
- [Using Global Node IDs - GitHub Docs](https://docs.github.com/en/graphql/guides/using-global-node-ids) - Node ID format and usage
- [GitHub GraphQL Cheatsheet](https://medium.com/@tharshita13/github-graphql-api-cheatsheet-38e916fe76a3) - Examples of PR queries with id field
- [About Protected Branches - GitHub Docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches) - Permission and bypass rules

### Tertiary (LOW confidence)
- [Best Practices for Reviewing Pull Requests](https://rewind.com/blog/best-practices-for-reviewing-pull-requests-in-github/) - UI patterns for review submission
- [How Teams Speed Up GitHub PR Reviews](https://www.codeant.ai/blogs/github-code-reviews) - 2026 workflow patterns
- [Stale Review Dismissal Discussion](https://github.com/orgs/community/discussions/12876) - Edge cases with stale reviews

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Using existing ISL patterns and official GitHub GraphQL API
- Architecture: HIGH - All patterns proven in existing codebase, clear integration points
- Pitfalls: MEDIUM - Based on API documentation and community discussions, some edge cases unverified
- GitHub API specifics: HIGH - Verified against generated GraphQL types and official documentation
- UI patterns: HIGH - Direct references to existing ISL components and patterns

**Research date:** 2026-02-02
**Valid until:** 2026-03-02 (30 days - stable GitHub GraphQL API)
