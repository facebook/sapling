# Phase 13: Sync/Rebase - Research

**Researched:** 2026-02-02
**Domain:** Git/Sapling sync operations, rebase workflows, GitHub PR updates
**Confidence:** HIGH

## Summary

Phase 13 enables users to keep PRs in sync with the latest main branch and rebase commit stacks without leaving review mode. The standard approach combines three technologies:

1. **GitHub CLI `gh pr update-branch`** for PR-level sync with remote main
2. **Sapling `sl rebase` operations** for local commit stack rebasing
3. **Existing ISL operation queue** for progress feedback and conflict handling

The domain has two distinct workflows: simple fast-forward updates (no conflicts) and interactive conflict resolution (requires user intervention). The existing codebase already has robust patterns for both: the operation queue handles async command execution with progress tracking, and the merge conflicts UI provides file-by-file resolution with checkmarks.

**Primary recommendation:** Use `gh pr update-branch --rebase` for single PR updates, `sl rebase -s 'draft()' -d 'main()'` for local stack rebasing, and extend existing operation/conflict infrastructure rather than building new progress UI.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| GitHub CLI (gh) | Latest | PR branch updates | Official GitHub tool, supports both merge and rebase strategies |
| Sapling CLI (sl) | Current | Local rebase operations | Built-in stack-aware rebasing with conflict detection |
| ISL OperationQueue | Existing | Async command execution | Already handles progress, queueing, and exit codes |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| Jotai atoms | Existing | State management | Tracking operation status, warnings, pending comments |
| serverAPI | Existing | Client-server RPC | Sending runOperation messages to backend |
| TypedEventEmitter | Existing | Progress events | Streaming stdout/stderr/progress from operations |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| gh CLI | GraphQL API | More control but requires auth management, conflict handling logic |
| Operation queue | Custom progress UI | Duplicates existing infrastructure, loses queueing benefits |
| Rebase operation | Manual git commands | Loses Sapling stack awareness and merge conflict integration |

**Installation:**
Already present in codebase. GitHub CLI required in PATH (documented in prerequisites).

## Architecture Patterns

### Recommended Integration Points

**Review Mode State:**
- Check for `currentReviewPR` in review mode atoms
- Extract PR number and head hash from GitHubDiffSummary
- Pass to sync/rebase operations

**Operation Classes:**
```typescript
// Existing pattern from RebaseOperation.ts
class SyncPROperation extends Operation {
  constructor(private prNumber: string) {
    super('SyncPROperation');
  }

  getArgs() {
    return ['gh', 'pr', 'update-branch', this.prNumber, '--rebase'];
  }
}

class RebaseStackOperation extends Operation {
  constructor() {
    super('RebaseStackOperation');
  }

  getArgs() {
    return ['rebase', '-s', 'draft()', '-d', 'main()'];
  }
}
```

### Pattern 1: Warning Before Sync
**What:** Check for pending comments and viewed file status before starting sync
**When to use:** Always, before any sync/rebase operation that may change commit hashes
**Example:**
```typescript
// Source: Project context - pendingCommentsState.ts + atoms.ts patterns
const pendingComments = readAtom(pendingCommentsAtom(prNumber));
const hasViewedFiles = // check reviewedFilesAtom for pr:${prNumber}:${headHash}:*

if (pendingComments.length > 0 || hasViewedFiles) {
  // Show warning modal with explanation:
  // - Pending comments may become invalid (line numbers shift)
  // - Viewed file status will reset (new headHash)
  // - User can choose to proceed or cancel
}
```

### Pattern 2: Operation Progress with Conflict Detection
**What:** Use existing OperationQueue + merge conflict detection
**When to use:** For all rebase operations (both PR-level and stack-level)
**Example:**
```typescript
// Source: Repository.ts - checkForMergeConflicts pattern
// Operation runs via useRunOperation hook
runOperation(new SyncPROperation(prNumber));

// Server-side: Repository.ts operationQueue automatically:
// 1. Runs command via runOperation
// 2. Streams progress via operationProgress messages
// 3. On exit, triggers watchForChanges.poll('force')
// 4. checkForMergeConflicts() detects .sl/merge directory
// 5. UI switches to conflict resolution mode

// Merge conflicts UI already exists (MergeConflicts.test.tsx)
// Shows list of files with status 'U' (unresolved)
// User resolves conflicts file-by-file
// Marks resolved with checkmark (ResolveOperation)
// Continues rebase with ContinueOperation
```

### Pattern 3: State Invalidation After Sync
**What:** Clear localStorage state tied to old commit hash
**When to use:** After successful PR sync (headHash changes)
**Example:**
```typescript
// Source: atoms.ts - reviewedFileKeyForPR pattern
// Key format: `pr:${prNumber}:${headHash}:${filePath}`

// After sync completes, GitHub API returns new headHash
// localStorage keys with old headHash naturally become stale
// No manual cleanup needed - they expire after 14 days

// Pending comments should be explicitly warned about before sync
// Option 1: Clear all pending comments on sync
// Option 2: Preserve them but warn they may be invalid
```

### Anti-Patterns to Avoid
- **Building custom progress UI:** Operation queue already provides standardized progress tracking with stdout/stderr streaming
- **Manual conflict resolution in UI:** Sapling CLI handles conflict markers, ISL just needs to detect and guide user
- **Syncing without warning:** Users must understand pending review state will be affected

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| PR branch updates | Custom git fetch + rebase | `gh pr update-branch --rebase` | Handles GitHub-specific logic (PR head tracking, force-push) |
| Progress feedback | Custom spinner/progress bar | OperationQueue + operationProgress events | Already streams stdout, detects exit codes, handles queueing |
| Conflict detection | Parse git status | Repository.checkForMergeConflicts() | Detects .sl/merge dir, fetches conflict details, emits events |
| Stack rebasing | Individual rebase commands | `sl rebase -s 'draft()' -d 'main()'` | Sapling revsets handle entire stack in one operation |
| Async command execution | Direct child_process spawning | Repository.runOrQueueOperation() | Handles cwd, env vars, timeouts, abort signals, optimistic state |

**Key insight:** The ISL architecture already solves async operation execution, progress tracking, and conflict resolution. The hard part (conflict detection, queueing, state management) is done. Phase 13 is primarily about:
1. Adding new Operation classes for sync/rebase
2. Adding UI buttons to trigger these operations
3. Warning users about state that will be invalidated

## Common Pitfalls

### Pitfall 1: Not Handling Merge Conflicts
**What goes wrong:** User clicks "Sync PR", rebase hits conflicts, operation appears stuck
**Why it happens:** GitHub CLI `gh pr update-branch --rebase` can fail with conflicts, requires local resolution
**How to avoid:**
- GitHub API limitation: "You aren't able to automatically rebase when the pull request has merge conflicts" ([GitHub CLI docs](https://cli.github.com/manual/gh_pr_update-branch))
- Use Sapling's conflict resolution: When operation exits non-zero, checkForMergeConflicts() runs
- UI switches to conflict resolution mode (existing pattern)
- User resolves conflicts file-by-file
- Continues with `sl continue` (ContinueOperation already exists)
**Warning signs:** Operation exits with code 1, stderr contains "conflict"

### Pitfall 2: Losing Pending Review Comments
**What goes wrong:** User syncs PR, line-based comments now point to wrong lines or deleted code
**Why it happens:** Rebase changes commit hashes and line numbers, but localStorage persists pending comments by PR number (not hash)
**How to avoid:**
- Before any sync operation, check `pendingCommentsAtom(prNumber).length > 0`
- If true, show warning modal: "You have X pending comments. Syncing may cause them to become invalid. Proceed?"
- Consider: Save pending comments before sync, attempt to map line numbers after sync (advanced, Phase 13 may defer)
**Warning signs:** User confusion when comments appear on wrong lines after sync

### Pitfall 3: Viewed File Status Confusion
**What goes wrong:** User marks files as reviewed, syncs PR, all files show as unreviewed again
**Why it happens:** `reviewedFileKeyForPR` includes headHash, new commit = new hash = new keys = all false
**How to avoid:**
- This is correct behavior (new commits should be reviewed)
- Warn user before sync: "Syncing will reset viewed file status for this PR"
- Don't try to migrate old viewed state (different commits = different files)
**Warning signs:** User reports "reviewed status disappeared"

### Pitfall 4: Concurrent Operations During Sync
**What goes wrong:** User starts sync, then tries to add a comment or submit review
**Why it happens:** Sync operation may take 30+ seconds for large stacks with conflicts
**How to avoid:**
- Operation queue already prevents concurrent operations (queuedOperations atom)
- Disable review UI actions while isOperationRunningAtom is true
- Show "Syncing..." state in review mode header
**Warning signs:** Race conditions, corrupted state, confusing UI

### Pitfall 5: Not Refreshing PR Data After Sync
**What goes wrong:** User syncs PR, but UI still shows old head hash and commit count
**Why it happens:** Sync operation completes, but diffSummaries cache not invalidated
**How to avoid:**
- Repository.ts already handles this: `watchForChanges.poll('force')` after operation exits
- This triggers `fetchSmartlogCommits()` and `codeReviewProvider.triggerDiffSummariesFetch()`
- Ensure operation exits cleanly (code 0 or error) to trigger refresh
**Warning signs:** UI doesn't update after sync completes

## Code Examples

Verified patterns from official sources:

### Example 1: Sync Single PR with GitHub CLI
```bash
# Source: https://cli.github.com/manual/gh_pr_update-branch
# Rebase PR #123 onto latest base branch (main)
gh pr update-branch 123 --rebase

# Exit codes:
# 0 = success
# 1 = conflicts or error
# stderr contains error message
```

### Example 2: Rebase Local Stack in Sapling
```bash
# Source: https://sapling-scm.com/docs/overview/rebase/
# Rebase all draft commits onto main
sl rebase -s 'draft()' -d 'main()'

# Revset 'draft()' = all unpublished commits
# Revset 'main()' = main bookmark/branch
# Preserves stack relationships
```

### Example 3: Conflict Resolution Workflow
```typescript
// Source: Repository.ts - checkForMergeConflicts()
// Sapling automatically detects conflicts after rebase

// 1. Operation exits (code 0 or non-zero)
// 2. watchForChanges.poll('force') triggers
// 3. checkForMergeConflicts() runs:
const mergeDirExists = await exists(path.join(this.info.dotdir, 'merge'));
if (mergeDirExists) {
  // Fetch conflict details
  const output = await this.runCommand(
    ['resolve', '--tool', 'internal:dumpjson', '--all']
  );
  // Parse and emit to UI
  this.mergeConflictsEmitter.emit('change', conflicts);
}

// 4. UI shows conflict resolution interface
// 5. User resolves file by file (ResolveOperation)
// 6. User continues (ContinueOperation: ['continue'])
```

### Example 4: Operation with Progress Feedback
```typescript
// Source: operationsState.ts patterns
class SyncPROperation extends Operation {
  constructor(private prNumber: string) {
    super('SyncPROperation');
  }

  static opName = 'Sync PR';

  getArgs() {
    return ['gh', 'pr', 'update-branch', this.prNumber, '--rebase'];
  }

  // Optional: Show initial progress
  getInitialInlineProgress(): Array<[string, string]> {
    return [[this.prNumber, 'syncing with main...']];
  }
}

// Usage:
const runOperation = useRunOperation();
runOperation(new SyncPROperation('123'));

// Progress automatically tracked via operationList atom
// stdout/stderr streamed to UI
// Exit code triggers refresh or conflict detection
```

### Example 5: Warning Before Sync
```typescript
// Source: Project patterns from pendingCommentsState.ts
function warnBeforeSync(prNumber: string, headHash: string): boolean {
  const pendingComments = readAtom(pendingCommentsAtom(prNumber));
  const hasReviewState = /* check localStorage for keys with headHash */;

  if (pendingComments.length === 0 && !hasReviewState) {
    return true; // Safe to proceed
  }

  // Show modal with checkboxes:
  // [ ] I understand pending comments may become invalid
  // [ ] I understand viewed file status will be reset
  // [Proceed] [Cancel]

  return userConfirmed;
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual rebase in terminal | ISL drag-and-drop rebase | 2022 (ISL v1) | Reduced rebase errors, better UX |
| git rebase commands | Sapling revsets | 2022 (Sapling launch) | Stack-aware rebasing, safer |
| Custom progress parsing | IPC progress_bar_update | 2023 | Real-time progress with topics/units |
| gh pr merge --rebase | gh pr update-branch --rebase | 2023 (GitHub CLI v2.32+) | Separate update from merge, clearer intent |
| Manual conflict markers | internal:union/merge-local/merge-other | Existing | Automated resolution strategies |

**Deprecated/outdated:**
- Direct git commands: Use Sapling CLI for stack operations
- Parsing stdout for progress: Use IPC messages (already implemented in Repository.ts)
- Custom GraphQL for PR updates: Use `gh` CLI which handles auth, retries, error messages

## Open Questions

Things that couldn't be fully resolved:

1. **Rebase All Open PRs in Stack**
   - What we know: GitHub CLI can update one PR at a time (`gh pr update-branch <number>`)
   - What's unclear: Should we update all PRs in stack sequentially, or just the bottom PR?
   - Recommendation: Start with single PR update (SYN-01). Multi-PR stack rebase (SYN-02) may require:
     - Parse stack info from PR body (GitHubDiffSummary.stackInfo already available)
     - Update each PR bottom-to-top
     - Handle case where middle PR conflicts (continue? abort stack?)

2. **Pending Comment Line Number Migration**
   - What we know: Comments stored with line numbers, rebase changes lines
   - What's unclear: Should we attempt to map line numbers to new commit?
   - Recommendation: Phase 13 warns and proceeds. Future enhancement could use diff hunks to map lines, but complex (different files may be affected).

3. **Force Push Handling**
   - What we know: `gh pr update-branch --rebase` force-pushes to PR branch
   - What's unclear: What if local changes exist that haven't been pushed?
   - Recommendation: Detect uncommitted changes before sync (existing pattern), warn user that remote will be force-pushed (expected behavior for rebase).

## Sources

### Primary (HIGH confidence)
- [GitHub CLI Manual: gh pr update-branch](https://cli.github.com/manual/gh_pr_update-branch) - Official command documentation
- [Sapling Docs: Rebase](https://sapling-scm.com/docs/overview/rebase/) - Stack rebasing patterns
- [Sapling Docs: Interactive Smartlog](https://sapling-scm.com/docs/addons/isl/) - Conflict resolution UI
- Repository.ts (addons/isl-server/src/Repository.ts) - Operation execution and conflict detection
- operationsState.ts (addons/isl/src/operationsState.ts) - Progress tracking patterns

### Secondary (MEDIUM confidence)
- [GitHub Docs: Resolving merge conflicts after rebase](https://docs.github.com/en/get-started/using-git/resolving-merge-conflicts-after-a-git-rebase) - Conflict resolution workflow
- [Material UI: Progress Components](https://mui.com/material-ui/react-progress/) - UI patterns (2026)
- [Best Practices for Async State](https://blog.pixelfreestudio.com/best-practices-for-handling-async-state-in-frontend-apps/) - Loading indicators

### Tertiary (LOW confidence)
- WebSearch findings about localStorage cache invalidation strategies - needs verification with project patterns

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - GitHub CLI and Sapling commands documented and stable
- Architecture: HIGH - Existing operation patterns proven in codebase (RebaseOperation, PullOperation)
- Pitfalls: HIGH - Derived from GitHub CLI limitations and Sapling conflict handling docs
- Multi-PR stack rebase: MEDIUM - Requires design decisions about error handling

**Research date:** 2026-02-02
**Valid until:** 60 days (stable domain - git/Sapling semantics change slowly)
