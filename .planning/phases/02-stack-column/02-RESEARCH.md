# Phase 2: Stack Column - Research

**Researched:** 2026-01-22
**Domain:** Interactive React list components with click-to-checkout functionality
**Confidence:** HIGH

## Summary

Phase 2 adds click-to-checkout functionality to the stack column, allowing users to navigate the PR stack by clicking any commit/PR to check it out. This research covers existing ISL patterns for commit interaction, operation execution, visual feedback, and UI structure for implementing clickable stack items with proper hover states and loading indicators.

ISL already has a well-established pattern for checkout operations via `GotoOperation`, with existing visual feedback systems (optimistic updates, inline progress), and sophisticated state management using Jotai atoms. The codebase follows React best practices with TypeScript, StyleX for styling, and atomic state management. The existing `Commit.tsx` component provides a template for interactive commit rows with hover states, context menus, and action buttons.

**Primary recommendation:** Extend the existing `Commit` component pattern with click-to-checkout functionality, reuse `GotoOperation` and `PullRevOperation` for checkout logic, and apply consistent hover/feedback patterns already established in the commit tree.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| React | 18.x | UI framework | Already used throughout ISL, concurrent features |
| Jotai | 2.x | State management | ISL's chosen atomic state library, used extensively |
| TypeScript | 5.x | Type safety | Enforced throughout codebase |
| StyleX | Latest | CSS-in-JS | ISL's styling solution, type-safe styles |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| isl-components | Local | UI primitives | Button, Tooltip, Icon - always use these |
| @stylexjs/stylex | Latest | Style definitions | Define component styles |
| jotai/utils | 2.x | Storage atoms | LocalStorage-backed atoms for persistence |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Jotai | Redux Toolkit | More boilerplate, unnecessary for atomic updates |
| StyleX | Emotion/styled-components | Inconsistent with codebase standards |
| Custom hooks | React Context | Jotai provides better granular reactivity |

**Installation:**
No additional packages needed - all required libraries already in use.

## Architecture Patterns

### Recommended Project Structure
```
addons/isl/src/
├── operations/          # Operation classes (GotoOperation, PullRevOperation)
├── Commit.tsx          # Main commit component (extend for click-to-checkout)
├── CommitTreeList.tsx  # Commit tree rendering
├── operationsState.ts  # useRunOperation hook
└── *.css              # Component styles
```

### Pattern 1: Click-to-Checkout Operation
**What:** Make entire stack item row clickable to trigger checkout
**When to use:** For navigating the stack by clicking commits/PRs
**Example:**
```typescript
// Source: Existing pattern from Commit.tsx lines 889-910
async function gotoAction(runOperation: ReturnType<typeof useRunOperation>, commit: CommitInfo) {
  const shouldProceed = await runWarningChecks([
    () => maybeWarnAboutRebaseOntoMaster(commit),
    () => maybeWarnAboutOldDestination(commit),
    () => maybeWarnAboutRebaseOffWarm(commit),
  ]);

  if (!shouldProceed) {
    return;
  }

  const dest =
    // If the commit has a remote bookmark, use that instead of the hash
    commit.remoteBookmarks.length > 0
      ? succeedableRevset(commit.remoteBookmarks[0])
      : latestSuccessorUnlessExplicitlyObsolete(commit);
  runOperation(new GotoOperation(dest));
  writeAtom(selectedCommits, new Set());
}
```

### Pattern 2: Operation Execution with useRunOperation
**What:** Execute Sapling operations through React hook
**When to use:** Any time you need to run an operation (goto, pull, etc.)
**Example:**
```typescript
// Source: operationsState.ts usage pattern
const runOperation = useRunOperation();

// For checkout:
runOperation(new GotoOperation(commitHash));

// For pull then checkout (main branch):
runOperation(new PullOperation());
// followed by goto, or combined in sequence
```

### Pattern 3: Hover States with CSS
**What:** Show visual feedback on hover for clickable items
**When to use:** Interactive list items, buttons that appear on hover
**Example:**
```css
/* Source: CommitTreeList.css lines 246-260 */
.goto-button {
  opacity: 0;
  transition: opacity 0.1s;
}

.commit:hover .goto-button {
  opacity: 1;
}
```

### Pattern 4: Jotai Atom for Operational State
**What:** Use atoms for reactive state management
**When to use:** Any state that needs to be shared or persisted
**Example:**
```typescript
// Source: StackActions.tsx lines 42-53
export const collapsedStacksAtom = atomWithStorage<string[]>(
  'isl.collapsedStacks',
  [],
);

export const isStackCollapsedAtom = atom(get => {
  const collapsed = get(collapsedStacksAtom);
  return (hash: string) => collapsed.includes(hash);
});
```

### Pattern 5: Fixed Header with Scrollable Content
**What:** Position element with position: sticky at top, content scrolls below
**When to use:** Main branch section at top of stack column
**Example:**
```css
/* Modern sticky positioning pattern */
.stack-main-section {
  position: sticky;
  top: 0;
  z-index: 10;
  background: var(--background);
}

.stack-items-container {
  overflow-y: auto;
  /* Parent must not have overflow: hidden */
}
```

### Anti-Patterns to Avoid
- **Don't use inline event handlers with arrow functions unless necessary:** Pass function references directly (onClick={handleClick} not onClick={() => handleClick()}) unless you need to pass arguments
- **Don't use position: fixed without accounting for parent overflow:** Use position: sticky which preserves document flow
- **Don't create state outside Jotai atoms:** ISL uses Jotai throughout - don't introduce useState for shared state
- **Don't manually manage loading states:** Operations system handles progress automatically via `inlineProgressByHash`

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Checkout operation | Custom git/sl commands | `GotoOperation` | Handles warnings, optimistic state, successor commits |
| Pull operation | Custom pull logic | `PullOperation` or `PullRevOperation` | Integrated with operation queue, progress tracking |
| Visual feedback during ops | Custom loading spinners | `inlineProgressByHash` atom | Automatically managed by operations system |
| Click + event propagation | Manual stopPropagation | Existing `useCommitCallbacks` pattern | Handles selection, double-click to drawer, context menu |
| Hover state management | JavaScript hover tracking | CSS :hover pseudo-class | Better performance, no JS overhead |
| Persistent UI state | localStorage directly | `atomWithStorage` from jotai/utils | Type-safe, reactive, handles serialization |

**Key insight:** ISL has sophisticated operation execution infrastructure. Don't bypass it - use `GotoOperation` through `useRunOperation` which handles queuing, progress, optimistic updates, and error handling automatically.

## Common Pitfalls

### Pitfall 1: Event Propagation Conflicts
**What goes wrong:** Click on commit row also triggers child button clicks, or prevents selection
**Why it happens:** React's event system bubbles events upward through component hierarchy
**How to avoid:** Use `event.stopPropagation()` in child button handlers (see Commit.tsx line 434)
**Warning signs:** Clicking a button triggers both button action and row click action

### Pitfall 2: Missing previewPreventsActions Check
**What goes wrong:** User can click to checkout during a drag-to-rebase preview, breaking UI state
**Why it happens:** Preview states render commits in temporary "what-if" state that shouldn't be actionable
**How to avoid:** Check `previewPreventsActions(previewType)` before allowing clicks (Commit.tsx line 106-119)
**Warning signs:** Operations can be triggered during previews, causing state confusion

### Pitfall 3: Not Using succeedableRevset for Remote Bookmarks
**What goes wrong:** Checkout uses commit hash when remote bookmark exists, losing readability
**Why it happens:** Missing the pattern of preferring remote bookmark names over hashes
**How to avoid:** Check `commit.remoteBookmarks.length > 0` first, use `succeedableRevset()` helper (Commit.tsx line 903-905)
**Warning signs:** Command history shows hashes instead of "main" or "origin/main"

### Pitfall 4: Forgetting Warning Checks
**What goes wrong:** Users checkout to distant commits without being warned, causing slow operations
**Why it happens:** GotoOperation can be called directly without the warning checks wrapper
**How to avoid:** Always call `runWarningChecks()` before `runOperation(new GotoOperation())` (Commit.tsx line 890-894)
**Warning signs:** Users complain about slow checkouts or unexpected behavior

### Pitfall 5: position: sticky Not Working
**What goes wrong:** Fixed header doesn't stick, scrolls away with content
**Why it happens:** Parent element has `overflow: hidden` which breaks sticky positioning
**How to avoid:** Ensure parent containers don't have overflow: hidden, use overflow: visible or auto on scrollable container
**Warning signs:** Sticky element scrolls away like normal element

### Pitfall 6: Not Clearing Selection After Goto
**What goes wrong:** After clicking to checkout, selected commit state remains stale
**Why it happens:** Selection is independent state not automatically cleared by operations
**How to avoid:** Call `writeAtom(selectedCommits, new Set())` after goto operation (Commit.tsx line 909)
**Warning signs:** Commit appears selected but is no longer current commit

## Code Examples

Verified patterns from official sources:

### Making Row Clickable (Existing Pattern)
```typescript
// Source: CommitTreeList.tsx lines 143-151
function useExtraCommitRowProps(info: DagCommitInfo): React.HTMLAttributes<HTMLDivElement> | void {
  const {isSelected, onClickToSelect, onDoubleClickToShowDrawer} = useCommitCallbacks(info);

  return {
    onClick: onClickToSelect,
    onDoubleClick: onDoubleClickToShowDrawer,
    className: isSelected ? 'commit-row-selected' : '',
  };
}
```

### Goto Button Click Handler (Button Stops Propagation)
```typescript
// Source: Commit.tsx lines 421-441
if (!actionsPrevented && !commit.isDot) {
  commitActions.push(
    <span className="goto-button" key="goto-button">
      <Tooltip title={t('Update files...')} delayMs={250}>
        <Button
          aria-label={t('Go to commit "$title"', {replace: {$title: commit.title}})}
          xstyle={styles.gotoButton}
          onClick={async event => {
            event.stopPropagation(); // Prevent row click
            await gotoAction(runOperation, commit);
          }}>
          <T>Goto</T>
          <Icon icon="newline" />
        </Button>
      </Tooltip>
    </span>,
  );
}
```

### Combined Pull + Goto for Main Branch
```typescript
// Pattern: Pull then checkout to main/origin
// Combine PullOperation with GotoOperation
const runOperation = useRunOperation();

async function goToMain() {
  // Pull latest from remote
  await runOperation(new PullOperation());

  // Then checkout to main
  runOperation(new GotoOperation('main'));
}
```

### Hover State with Visual Feedback
```css
/* Source: CommitTreeList.css and ISL patterns */
.stack-item {
  cursor: pointer;
  transition: background-color 0.1s;
}

.stack-item:hover {
  background-color: var(--hover-darken);
}

.stack-item.checked-out {
  background-color: var(--selected-commit-background);
}
```

### Fixed Main Section at Top
```css
/* Sticky positioning for main branch section */
.stack-main-section {
  position: sticky;
  top: 0;
  z-index: 10;
  background-color: var(--background);
  padding: var(--pad);
  border-bottom: 1px solid var(--graphite-border);
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| useState for shared state | Jotai atoms | ISL rewrite | Fine-grained reactivity, better performance |
| CSS Modules | StyleX | Recent | Type-safe styles, better tree-shaking |
| position: fixed | position: sticky | CSS3 standard | Simpler implementation, preserves document flow |
| Manual loading states | Operation system | ISL architecture | Automatic progress tracking, optimistic updates |
| Context API | Jotai | ISL rewrite | Atomic updates, no provider nesting |

**Deprecated/outdated:**
- Manual localStorage usage: Use `atomWithStorage` instead
- Direct DOM manipulation: React patterns only
- Global CSS classes: Use StyleX for component styles
- useState + Context: Use Jotai atoms for shared state

## Open Questions

Things that couldn't be fully resolved:

1. **Exact visual treatment for origin/main marker in commit history**
   - What we know: Should be "moderately prominent" per CONTEXT.md
   - What's unclear: Specific color, icon, or badge style
   - Recommendation: Use similar styling to stable commit metadata (Tag component with distinct color from graphiteColors palette)

2. **Combined pull+checkout vs separate operations**
   - What we know: Main branch needs "go to main" (pull and checkout in one click)
   - What's unclear: Whether to combine into single operation or chain operations
   - Recommendation: Chain operations (PullOperation followed by GotoOperation) for better operation history and potential to interrupt

3. **Sync status display for main branch**
   - What we know: CONTEXT.md specifies showing "behind by X commits" or "up to date"
   - What's unclear: Exact API for getting sync status with remote main
   - Recommendation: Investigate `syncStatusAtom` and remote bookmark data from `dagWithPreviews`

## Sources

### Primary (HIGH confidence)
- ISL codebase: `addons/isl/src/Commit.tsx` - gotoAction pattern (lines 889-910)
- ISL codebase: `addons/isl/src/operations/GotoOperation.ts` - Operation implementation
- ISL codebase: `addons/isl/src/operations/PullOperation.ts` - Pull operation
- ISL codebase: `addons/isl/src/operationsState.ts` - useRunOperation hook
- ISL codebase: `addons/isl/src/CommitTreeList.tsx` - useExtraCommitRowProps pattern
- ISL codebase: `addons/components/theme/tokens.stylex.ts` - StyleX tokens and spacing

### Secondary (MEDIUM confidence)
- [React onClick patterns - MDN](https://developer.mozilla.org/en-US/docs/Learn_web_development/Core/Frameworks_libraries/React_interactivity_events_state)
- [CSS sticky positioning - DEV Community](https://dev.to/luisaugusto/stop-using-fixed-headers-and-start-using-sticky-ones-1k30)
- [Jotai documentation](https://jotai.org) - Official docs for atomic state management
- [React Loading State Patterns - Medium](https://medium.com/uxdworld/6-loading-state-patterns-that-feel-premium-716aa0fe63e8)

### Tertiary (LOW confidence)
- [CSS Hover Effects 2026 - FreeFrontend](https://freefrontend.com/css-hover-effects/) - General hover patterns
- [React Best Practices 2026 - Technostacks](https://technostacks.com/blog/react-best-practices/) - General React guidance

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries verified in use in ISL codebase
- Architecture: HIGH - Patterns extracted directly from ISL source code
- Pitfalls: HIGH - Identified from actual code patterns and anti-patterns in codebase

**Research date:** 2026-01-22
**Valid until:** ~30 days (stable patterns, mature codebase)
