# Phase 12: Merge + CI Status - Research

**Researched:** 2026-02-02
**Domain:** GitHub PR Merging, CI Status Display, Mergeability Checks
**Confidence:** HIGH

## Summary

This phase adds merge capabilities and CI status display to the review mode established in Phase 9. The existing codebase already fetches CI status via `statusCheckRollup.state` in the `YourPullRequestsQuery` GraphQL query and displays it as `signalSummary` on the `DiffSummary` type. The implementation needs to: (1) extend this to show detailed CI status in review mode, (2) add mergeability checks from additional PR fields, and (3) implement merge operations via `gh pr merge` CLI or GraphQL mutation.

The research confirms that GitHub provides three merge methods: MERGE, SQUASH, and REBASE. The `gh pr merge` CLI command provides a straightforward way to execute merges with strategy selection (`--merge`, `--squash`, `--rebase`). The GraphQL API offers `mergePullRequest` mutation for finer control but requires PR node ID. Repository settings determine which merge methods are allowed (`mergeCommitAllowed`, `squashMergeAllowed`, `rebaseMergeAllowed`).

**Primary recommendation:** Use `gh pr merge` CLI via server-side operation for merge execution, extend existing `signalSummary` to detailed CI view, and add mergeability fields to GraphQL queries.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Jotai | existing | State management for merge UI | Already used throughout ISL, atomFamily for PR state |
| gh CLI | existing | Execute merge commands | Already used for GraphQL, handles auth, simpler than raw API |
| GitHub GraphQL | v4 | Fetch CI status, mergeability | Current queries already use it, just need additional fields |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| isl-components | existing | Button, Dropdown, Icon | Merge strategy UI, status indicators |
| Operation class | existing | Run merge as tracked operation | Progress feedback, error handling |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| gh pr merge CLI | GraphQL mergePullRequest | GraphQL needs node ID lookup, CLI handles more edge cases |
| Extend YourPullRequestsQuery | Separate mergeability query | Single query reduces latency, but larger payload |
| Custom CI UI | Reuse DiffSignalSummary | Existing component works, just needs expansion |

**Installation:**
```bash
# No new dependencies needed - all infrastructure exists
```

## Architecture Patterns

### Recommended Project Structure
```
addons/isl/src/
  reviewMode/                 # New directory for review mode extensions
    MergeControls.tsx         # Merge button with strategy dropdown
    CIStatusBadge.tsx         # Detailed CI status display
    mergeState.ts             # Jotai atoms for merge UI state
  operations/
    MergePROperation.ts       # New operation for gh pr merge
  codeReview/github/
    github.tsx                # Extend with merge method helpers
addons/isl-server/src/github/
  githubCodeReviewProvider.ts # Extend to fetch mergeability data
  generated/graphql.ts        # Query already has status, add mergeability
```

### Pattern 1: Merge Operation via gh CLI
**What:** Create Operation subclass that runs `gh pr merge` with strategy flag
**When to use:** For merge execution (MRG-02)
**Example:**
```typescript
// Source: Pattern from addons/isl/src/operations/PullStackOperation.ts
import { Operation } from './Operation';

export type MergeStrategy = 'merge' | 'squash' | 'rebase';

export class MergePROperation extends Operation {
  static opName = 'MergePR';

  constructor(
    private prNumber: number,
    private strategy: MergeStrategy,
    private deleteBranch: boolean = false,
  ) {
    super('MergePROperation');
  }

  getArgs() {
    const args = ['pr', 'merge', String(this.prNumber)];
    args.push(`--${this.strategy}`);
    if (this.deleteBranch) {
      args.push('--delete-branch');
    }
    return args;
  }

  getDescriptionForDisplay() {
    return {
      description: `Merge PR #${this.prNumber} (${this.strategy})`,
    };
  }
}
```

### Pattern 2: Extended PR Data for Mergeability
**What:** Add mergeability fields to existing GraphQL query
**When to use:** For determining if merge button should be enabled (MRG-03)
**Example:**
```typescript
// Source: Pattern from addons/isl-server/src/github/generated/graphql.ts
// Extend YourPullRequestsQuery to include:

const additionalPRFields = `
  id                    # Node ID for GraphQL mutation fallback
  mergeable             # MergeableState: MERGEABLE, CONFLICTING, UNKNOWN
  mergeStateStatus      # MergeStateStatus: CLEAN, DIRTY, BLOCKED, etc.
  viewerCanMergeAsAdmin # Boolean: can bypass protections
  reviewDecision        # Already fetched: APPROVED, CHANGES_REQUESTED, etc.
  commits(last: 1) {
    nodes {
      commit {
        statusCheckRollup {
          state           # Already fetched: SUCCESS, FAILURE, PENDING, etc.
          contexts(first: 20) {
            nodes {
              ... on CheckRun {
                name
                conclusion
                status
                detailsUrl
              }
              ... on StatusContext {
                context
                state
                targetUrl
              }
            }
          }
        }
      }
    }
  }
`;
```

### Pattern 3: Mergeability State Derivation
**What:** Combine multiple fields to determine if PR can be merged
**When to use:** For MRG-03 merge button disabled state
**Example:**
```typescript
// Source: Logic based on GitHub's merge rules
export type MergeabilityStatus = {
  canMerge: boolean;
  reasons: string[];  // Why merge is blocked
};

export function deriveMergeability(pr: ExtendedPRSummary): MergeabilityStatus {
  const reasons: string[] = [];

  // Check CI status
  if (pr.signalSummary === 'failed') {
    reasons.push('CI checks are failing');
  } else if (pr.signalSummary === 'running') {
    reasons.push('CI checks are still running');
  }

  // Check review decision
  if (pr.reviewDecision === 'CHANGES_REQUESTED') {
    reasons.push('Changes have been requested');
  } else if (pr.reviewDecision === 'REVIEW_REQUIRED') {
    reasons.push('Review approval is required');
  }

  // Check merge conflicts
  if (pr.mergeable === 'CONFLICTING') {
    reasons.push('Merge conflicts exist');
  }

  // Check merge state status
  if (pr.mergeStateStatus === 'BLOCKED') {
    reasons.push('Merge is blocked by branch protection rules');
  } else if (pr.mergeStateStatus === 'BEHIND') {
    reasons.push('Branch is behind base branch');
  }

  return {
    canMerge: reasons.length === 0,
    reasons,
  };
}
```

### Pattern 4: CI Status Detail Component
**What:** Expandable CI status showing individual check runs
**When to use:** For MRG-01 detailed CI view
**Example:**
```typescript
// Source: Pattern from addons/isl/src/codeReview/DiffBadge.tsx DiffSignalSummary
// Extend existing component pattern

type CheckRunStatus = {
  name: string;
  status: 'COMPLETED' | 'IN_PROGRESS' | 'QUEUED';
  conclusion?: 'SUCCESS' | 'FAILURE' | 'NEUTRAL' | 'CANCELLED' | 'TIMED_OUT';
  detailsUrl?: string;
};

function CIStatusDetail({ checks }: { checks: CheckRunStatus[] }) {
  const passing = checks.filter(c => c.conclusion === 'SUCCESS');
  const failing = checks.filter(c => c.conclusion === 'FAILURE');
  const running = checks.filter(c => c.status !== 'COMPLETED');

  return (
    <div className="ci-status-detail">
      {failing.length > 0 && (
        <div className="ci-failing">
          <Icon icon="error" /> {failing.length} failing
        </div>
      )}
      {running.length > 0 && (
        <div className="ci-running">
          <Icon icon="loading" /> {running.length} running
        </div>
      )}
      {passing.length > 0 && (
        <div className="ci-passing">
          <Icon icon="check" /> {passing.length} passing
        </div>
      )}
    </div>
  );
}
```

### Anti-Patterns to Avoid
- **Using GraphQL mutation without CLI fallback:** `gh pr merge` handles edge cases like merge queues better
- **Polling for CI status:** Use existing subscription pattern from `fetchDiffSummaries`
- **Ignoring merge queue:** Some repos require merge queue - `gh` CLI handles this transparently
- **Custom merge conflict resolution:** Users should resolve in their editor, not in ISL

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| CI status rollup | Custom aggregation | `statusCheckRollup.state` | Already computed by GitHub |
| Merge execution | GraphQL mutation | `gh pr merge` CLI | Handles merge queues, auth, edge cases |
| Status icons | Custom SVG | Existing `DiffSignalSummary` icons | Already styled, consistent |
| Operation progress | Custom tracking | `Operation` class | Built-in progress, cancel, error handling |
| Merge strategy list | Hardcoded | Query `Repository.{merge,squash,rebase}MergeAllowed` | Repos can disable strategies |

**Key insight:** The existing `signalSummary` on `DiffSummary` already maps GitHub's `StatusState` to ISL's signal types. Extend this pattern rather than creating parallel CI status handling.

## Common Pitfalls

### Pitfall 1: Ignoring Merge Queue Requirements
**What goes wrong:** Merge fails with "This repository requires a merge queue"
**Why it happens:** Some repos have branch protection requiring merge queue
**How to avoid:** Use `gh pr merge --auto` when merge queue is enabled, or `enqueuePullRequest` GraphQL mutation
**Warning signs:** Repo has `mergeQueueEntry` field populated for other PRs

### Pitfall 2: Stale Mergeability State
**What goes wrong:** User clicks merge but PR is no longer mergeable
**Why it happens:** PR state changed since last fetch
**How to avoid:** Re-fetch PR state before merge, use `expectedHeadOid` to detect race conditions
**Warning signs:** Merge fails with "Head ref was updated"

### Pitfall 3: Missing Repository Merge Settings
**What goes wrong:** Offer squash merge when repo only allows rebase
**Why it happens:** Didn't check repository's allowed merge methods
**How to avoid:** Query `Repository.{merge,squash,rebase}MergeAllowed` and only show allowed options
**Warning signs:** Merge fails with "Merge method not allowed"

### Pitfall 4: CI Status Caching Issues
**What goes wrong:** Showing outdated CI status after new commits
**Why it happens:** CI status cached from previous query
**How to avoid:** Invalidate on PR update (new `head` hash), or refresh before merge
**Warning signs:** CI shows green but merge blocked by failing checks

### Pitfall 5: Not Handling Admin Merge Bypass
**What goes wrong:** Admin users can't merge blocked PRs even when they should be able to
**Why it happens:** Didn't check `viewerCanMergeAsAdmin` field
**How to avoid:** Show "Merge as Admin" option when user has bypass permission
**Warning signs:** Admin complains merge button is disabled

## Code Examples

Verified patterns from official sources and existing ISL code:

### Existing CI Status Mapping
```typescript
// Source: addons/isl-server/src/github/githubCodeReviewProvider.ts:439-452
function githubStatusRollupStateToCIStatus(state: StatusState | undefined): DiffSignalSummary {
  switch (state) {
    case undefined:
    case StatusState.Expected:
      return 'no-signal';
    case StatusState.Pending:
      return 'running';
    case StatusState.Error:
    case StatusState.Failure:
      return 'failed';
    case StatusState.Success:
      return 'pass';
  }
}
```

### Existing DiffSummary Type with Signal
```typescript
// Source: addons/isl-server/src/github/githubCodeReviewProvider.ts:48-71
export type GitHubDiffSummary = {
  type: 'github';
  title: string;
  state: PullRequestState | 'DRAFT' | 'MERGE_QUEUED';
  number: DiffId;
  signalSummary?: DiffSignalSummary;  // CI status already here!
  reviewDecision?: PullRequestReviewDecision;
  // ... extend with mergeability fields
};
```

### GitHub Merge Method Enum
```typescript
// Source: addons/isl-server/src/github/generated/graphql.ts:17854-17861
export enum PullRequestMergeMethod {
  /** Add all commits from head to base with a merge commit. */
  Merge = 'MERGE',
  /** Add all commits from head onto base individually. */
  Rebase = 'REBASE',
  /** Combine all commits into single commit in base. */
  Squash = 'SQUASH'
}
```

### MergeableState and MergeStateStatus Enums
```typescript
// Source: addons/isl-server/src/github/generated/graphql.ts:10369-10375
export enum MergeableState {
  /** Cannot merge due to conflicts. */
  Conflicting = 'CONFLICTING',
  /** Can be merged. */
  Mergeable = 'MERGEABLE',
  /** Mergeability not yet computed. */
  Unknown = 'UNKNOWN'
}

// Source: addons/isl-server/src/github/generated/graphql.ts:10346-10365
export enum MergeStateStatus {
  Behind = 'BEHIND',       // Head ref out of date
  Blocked = 'BLOCKED',     // Blocked by branch protection
  Clean = 'CLEAN',         // Ready to merge
  Dirty = 'DIRTY',         // Merge conflicts
  Draft = 'DRAFT',         // PR is draft
  HasHooks = 'HAS_HOOKS',  // Pre-receive hooks
  Unknown = 'UNKNOWN',     // Not yet computed
  Unstable = 'UNSTABLE'    // Required checks not passing
}
```

### Existing Operation Pattern
```typescript
// Source: addons/isl/src/operations/PrSubmitOperation.ts
export class PrSubmitOperation extends Operation {
  static opName = 'pr submit';

  constructor(private options?: {draft?: boolean; updateMessage?: string}) {
    super('PrSubmitOperation');
  }

  getArgs() {
    const args = ['pr', 'submit'];
    if (this.options?.draft) {
      args.push('--draft');
    }
    return args;
  }
}
```

### Toast for Feedback
```typescript
// Source: addons/isl/src/toast.ts
export function showToast(message: ReactNode, props?: {durationMs?: number; key?: string}) {
  // Use for merge success/failure feedback
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `sl pr merge` | `gh pr merge` | Current | Use gh CLI directly for merge |
| Individual status checks | `statusCheckRollup.state` | Current | Single field summarizes all checks |
| Poll for mergeability | Query with fetch | Current | Fetch on demand, not continuous polling |

**Deprecated/outdated:**
- Direct GitHub REST API merge endpoint: Use GraphQL or `gh` CLI instead
- Manual auth token handling: `gh` CLI manages authentication

## Open Questions

Things that couldn't be fully resolved:

1. **Merge Queue Detection**
   - What we know: `mergeQueueEntry` field exists, `gh pr merge --auto` works with queues
   - What's unclear: How to detect if repo requires merge queue before attempting merge
   - Recommendation: Attempt normal merge first, show queue UI if needed

2. **Delete Branch After Merge**
   - What we know: `gh pr merge --delete-branch` exists
   - What's unclear: Should this be default or user choice?
   - Recommendation: Add checkbox, default to false to avoid surprises

3. **Repository Merge Settings Query**
   - What we know: `Repository.{merge,squash,rebase}MergeAllowed` fields exist
   - What's unclear: Whether to query this per-PR or cache per-repo
   - Recommendation: Query once per session, cache in Jotai atom

4. **Refresh After Merge**
   - What we know: After merge, PR state and commit tree change
   - What's unclear: Best UX for post-merge state update
   - Recommendation: Auto-refresh diff summaries, show success toast, exit review mode

## Sources

### Primary (HIGH confidence)
- `addons/isl-server/src/github/generated/graphql.ts` - Full GitHub GraphQL schema
- `addons/isl-server/src/github/githubCodeReviewProvider.ts` - Existing CI status handling
- `addons/isl/src/codeReview/DiffBadge.tsx` - Existing DiffSignalSummary component
- `addons/isl/src/operations/PrSubmitOperation.ts` - Operation pattern for CLI commands
- [GitHub CLI Manual: gh pr merge](https://cli.github.com/manual/gh_pr_merge)

### Secondary (MEDIUM confidence)
- [GitHub Docs: Input Objects - MergePullRequestInput](https://docs.github.com/en/graphql/reference/input-objects#mergepullrequestinput)
- [GitHub Docs: About pull request merges](https://docs.github.com/articles/about-pull-request-merges)
- [GitHub Changelog: Merge Queue API](https://github.blog/changelog/2023-04-19-pull-request-merge-queue-public-beta-api-support-and-recent-fixes/)

### Tertiary (LOW confidence)
- Community discussions on merge queue automation edge cases

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Using existing ISL patterns and gh CLI
- Architecture: HIGH - Extends proven patterns from Phase 9 and existing operations
- Pitfalls: HIGH - Based on documented GitHub API behavior and schema
- Merge queue handling: MEDIUM - Edge cases may need discovery

**Research date:** 2026-02-02
**Valid until:** 2026-03-02 (30 days - stable GitHub API, gh CLI)
