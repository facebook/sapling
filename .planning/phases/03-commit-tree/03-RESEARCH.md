# Phase 3: Commit Tree - Research

**Researched:** 2026-01-22
**Domain:** React UI patterns (scroll synchronization, avatar display, selection highlighting)
**Confidence:** HIGH

## Summary

This phase implements synchronized selection with author information and origin/main highlighting in the ISL commit tree (middle column). The research focuses on three technical domains: auto-scroll behavior using React refs and scrollIntoView, author avatar display with fallbacks, and VS Code-style selection highlighting.

**Key findings:**
- React 18 scrollIntoView with `behavior: 'smooth', block: 'center'` is the standard approach
- ISL already has avatar infrastructure (Avatar.tsx, fetchAvatars API)
- CSS variables for selection highlighting are established (`--selected-commit-background`, `--graphite-accent`)
- Data attribute `data-commit-hash` is already set on commit rows for targeting

**Primary recommendation:** Use useEffect with scrollIntoView for auto-scroll, extend existing Avatar component for author display, add left border accent to existing `.commit-row-selected` class for VS Code-style highlighting.

## Standard Stack

The ISL codebase already has established patterns for the required functionality:

### Core Libraries (Already in Use)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| React | 18.3.1 | UI framework | Selected by ISL, supports modern hooks |
| Jotai | 2.6.2 | State management | ISL's chosen atom-based state solution |
| @stylexjs/stylex | Latest | CSS-in-JS | ISL uses for component styling |

### Existing ISL Infrastructure
| Component/API | Location | Purpose | Current Usage |
|--------------|----------|---------|---------------|
| Avatar.tsx | addons/isl/src/Avatar.tsx | Avatar display with server API | Used in DAG rendering |
| fetchAvatars | ServerAPI message type | Fetches avatar URLs from server | Backend integration exists |
| data-commit-hash | RenderDag.tsx line 396 | Commit row identification | Already set on commit rows |
| selectedCommits atom | selection.ts | Selection state | Manages current selection |
| --graphite-accent | CSS variable | Accent color (#4a90e2) | Phase 1 established color |

**Installation:** No new dependencies required - all functionality uses existing ISL infrastructure.

## Architecture Patterns

### Recommended Project Structure
No structural changes needed - existing file organization is appropriate:
```
addons/isl/src/
├── CommitTreeList.tsx    # Add scroll logic here
├── Commit.tsx            # Add author display here
├── selection.ts          # Selection state (no changes)
├── Avatar.tsx            # Extend for commit author display
└── CommitTreeList.css    # Add selection highlight styles
```

### Pattern 1: Auto-Scroll on Selection
**What:** When selection changes, smoothly scroll the selected commit into view, centered in the viewport
**When to use:** On selection changes from any source (click, keyboard, stack selection sync)
**Example:**
```typescript
// Source: React best practices from multiple sources
// https://openillumi.com/en/en-react-useref-scrollintoview-control/
// https://www.codemzy.com/blog/react-scroll-to-element-on-render

function CommitTreeList() {
  const selectedHash = useAtomValue(selectedCommits);

  useEffect(() => {
    if (selectedHash.size === 1) {
      const hash = Array.from(selectedHash)[0];
      // Query by data-commit-hash attribute
      const element = document.querySelector(`[data-commit-hash="${hash}"]`);

      if (element) {
        // Small delay to ensure DOM has rendered
        setTimeout(() => {
          element.scrollIntoView({
            behavior: 'smooth',
            block: 'center',
            inline: 'nearest'
          });
        }, 100);
      }
    }
  }, [selectedHash]);
}
```

**Why setTimeout 100ms:**
- Ensures DOM has fully rendered after state update
- ComparisonView.tsx (line 113-122) uses this exact pattern for scrollToFile
- Alternative is requestAnimationFrame but setTimeout is simpler for one-off scrolls

### Pattern 2: Author Avatar Display
**What:** Display author avatar/initials next to commit info
**When to use:** For all commits (draft and public)
**Example:**
```typescript
// Based on existing Avatar.tsx pattern
// ISL already has this infrastructure

function DagCommitBody({info}: {info: DagCommitInfo}) {
  return (
    <div className="commit-details">
      <Avatar username={info.author} />
      <span className="commit-title">{info.title}</span>
      {/* rest of commit info */}
    </div>
  );
}
```

**Avatar fallback pattern:**
ISL's Avatar.tsx (lines 66-78) already implements fallback:
- Fetches URL via `fetchAvatars` server API
- Returns `<BlankAvatar />` if URL is null
- For initials with consistent colors, need to extend BlankAvatar

### Pattern 3: Initials Avatar with Consistent Colors
**What:** When avatar URL unavailable, show initials in colored circle with hash-based consistent color
**When to use:** Fallback when avatar fetch returns null
**Example:**
```typescript
// Based on research from multiple sources:
// https://dev.to/admitkard/auto-generate-avatar-colors-randomly-138j
// https://marcoslooten.com/blog/creating-avatars-with-colors-using-the-modulus/

function getConsistentColor(username: string): string {
  // Hash the username to a number
  let hash = 0;
  for (let i = 0; i < username.length; i++) {
    hash = username.charCodeAt(i) + ((hash << 5) - hash);
  }

  // Use modulo to select from color palette
  const colors = [
    '#e91e63', '#9c27b0', '#673ab7', '#3f51b5',
    '#2196f3', '#00bcd4', '#009688', '#4caf50',
    '#ff9800', '#ff5722', '#795548', '#607d8b'
  ];

  return colors[Math.abs(hash) % colors.length];
}

function InitialsAvatar({username}: {username: string}) {
  const initials = username.slice(0, 2).toUpperCase();
  const bgColor = getConsistentColor(username);

  return (
    <div className="avatar-initials" style={{backgroundColor: bgColor}}>
      {initials}
    </div>
  );
}
```

**Why hash-based colors:**
- Same username always gets same color (consistency)
- Visually distinct users in team context
- No server round-trip needed

### Pattern 4: VS Code-Style Selection Highlighting
**What:** Left border accent on selected commit row
**When to use:** When commit is selected (`.commit-row-selected` class already exists)
**Example:**
```css
/* Source: VSCode theme patterns
 * https://github.com/microsoft/vscode/issues/45479
 * ISL already uses similar pattern in PRDashboard.css lines 173-174
 */

.commit-row-selected {
  background-color: var(--selected-commit-background);
  border-left: 3px solid var(--graphite-accent, #4a90e2);
  margin-left: -3px; /* Prevent layout shift */
}

.commit-row-selected .commit-details {
  /* Subtle gradient like origin/main but for selection */
  background: linear-gradient(
    90deg,
    rgba(74, 144, 226, 0.08) 0%,
    transparent 50%
  );
}
```

**Why left border:**
- VS Code uses this pattern for file tree selection (user request in CONTEXT.md)
- Already established in PRDashboard.css for current PR row
- Doesn't interfere with commit DAG visualization on left

### Anti-Patterns to Avoid
- **Don't query by commit hash directly:** Use `[data-commit-hash]` attribute selector, not string matching on content
- **Don't scroll on every atom update:** Only scroll when selection actually changes (use proper useEffect deps)
- **Don't use random colors for avatars:** Must use hash-based deterministic colors for consistency
- **Don't block main thread:** Use setTimeout for scroll, not synchronous DOM manipulation

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Avatar fetching | Custom image loader | ISL's `fetchAvatars` API | Server already handles avatar URLs, caching, error handling |
| Scroll container detection | Manual parent traversal | `scrollIntoView` native API | Browser handles overflow detection, polyfills exist |
| Color hashing | Random colors | Simple charCode hash with modulo | Consistent colors per user, deterministic |
| Selection state | Local component state | Jotai `selectedCommits` atom | Already manages selection, succession tracking |
| Smooth scrolling | CSS transitions on scroll position | `behavior: 'smooth'` option | Native browser implementation, respects prefers-reduced-motion |

**Key insight:** ISL has mature infrastructure for all required functionality. The implementation is primarily composition and styling, not new primitives.

## Common Pitfalls

### Pitfall 1: Scroll Timing Race Conditions
**What goes wrong:** Calling scrollIntoView immediately after state update doesn't scroll to element
**Why it happens:** React batches updates, DOM may not reflect new selection when scrollIntoView is called
**How to avoid:** Use setTimeout with 100ms delay (proven pattern in ComparisonView.tsx)
**Warning signs:** Element exists in DOM but scroll doesn't happen, or scrolls to wrong location

**Code example:**
```typescript
// WRONG - may execute before React renders
useEffect(() => {
  const el = document.querySelector(`[data-commit-hash="${hash}"]`);
  el?.scrollIntoView(); // May not work
}, [hash]);

// CORRECT - wait for render
useEffect(() => {
  const timer = setTimeout(() => {
    const el = document.querySelector(`[data-commit-hash="${hash}"]`);
    el?.scrollIntoView({behavior: 'smooth', block: 'center'});
  }, 100);
  return () => clearTimeout(timer);
}, [hash]);
```

### Pitfall 2: Avatar Component Re-rendering on Every Commit
**What goes wrong:** Avatar component fetches on every render, causing excessive server requests
**Why it happens:** Avatar.tsx uses `lazyAtom` and `atomFamilyWeak` for caching, but parent re-renders can bypass cache
**How to avoid:** Use React.memo on commit components (already done in Commit.tsx line 154)
**Warning signs:** Network tab shows repeated fetchAvatars requests for same author

### Pitfall 3: Hash Function Collisions for Avatar Colors
**What goes wrong:** Different usernames get same color, reducing visual distinction
**Why it happens:** Poor hash function or small color palette
**How to avoid:**
- Use full username string for hash, not just initials
- Minimum 12 colors in palette (reduces collision to ~8%)
- Use charCodeAt with bit shifting for better distribution
**Warning signs:** Multiple team members have same avatar color

### Pitfall 4: Selection Border Causing Layout Shift
**What goes wrong:** Adding 3px left border pushes content right when selecting
**Why it happens:** Border adds to element width
**How to avoid:** Use negative margin to compensate: `margin-left: -3px`
**Warning signs:** Commit text jumps horizontally on selection

**Code example:**
```css
/* WRONG - causes layout shift */
.commit-row-selected {
  border-left: 3px solid var(--graphite-accent);
}

/* CORRECT - no layout shift */
.commit-row-selected {
  border-left: 3px solid var(--graphite-accent);
  margin-left: -3px;
}
```

### Pitfall 5: Scrolling on Initial Page Load Conflicts
**What goes wrong:** Page scrolls to HEAD commit before user has oriented themselves
**Why it happens:** useEffect runs on mount with current HEAD as selected
**How to avoid:** Add flag to track initial mount, only auto-scroll on selection *changes*
**Warning signs:** Page jumps to different location when loading

## Code Examples

Verified patterns from official sources and ISL codebase:

### Auto-Scroll Implementation
```typescript
// Source: ComparisonView.tsx lines 110-125 (verified ISL pattern)
// Adapted for commit selection

import {useEffect} from 'react';
import {useAtomValue} from 'jotai';
import {selectedCommits} from './selection';

export function useScrollToSelectedCommit() {
  const selected = useAtomValue(selectedCommits);

  useEffect(() => {
    // Only scroll when exactly one commit is selected
    if (selected.size !== 1) {
      return;
    }

    const hash = Array.from(selected)[0];

    // Small delay to ensure DOM has rendered
    const timer = setTimeout(() => {
      const element = document.querySelector(
        `[data-commit-hash="${hash}"]`
      );

      if (element) {
        element.scrollIntoView({
          behavior: 'smooth',
          block: 'center',
          inline: 'nearest'
        });
      }
    }, 100);

    return () => clearTimeout(timer);
  }, [selected]);
}

// Usage in CommitTreeList.tsx
export function CommitTreeList() {
  useScrollToSelectedCommit(); // Add this hook

  // ... rest of component
}
```

### Author Avatar with Initials Fallback
```typescript
// Source: Extended from Avatar.tsx (lines 66-78)
// Hash function from https://dev.to/admitkard/auto-generate-avatar-colors-randomly-138j

import {useAtomValue} from 'jotai';
import {avatarUrl} from './Avatar';

const AVATAR_COLORS = [
  '#e91e63', '#9c27b0', '#673ab7', '#3f51b5',
  '#2196f3', '#00bcd4', '#009688', '#4caf50',
  '#ff9800', '#ff5722', '#795548', '#607d8b'
];

function hashStringToColor(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    hash = str.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function getInitials(username: string): string {
  // Extract username from email if present (e.g., "user@example.com" -> "us")
  const name = username.split('@')[0];
  return name.slice(0, 2).toUpperCase();
}

function InitialsAvatar({username}: {username: string}) {
  const initials = getInitials(username);
  const bgColor = hashStringToColor(username);

  return (
    <div
      className="avatar-initials"
      style={{
        backgroundColor: bgColor,
        width: '20px',
        height: '20px',
        borderRadius: '50%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontSize: '10px',
        fontWeight: 600,
        color: 'white',
        flexShrink: 0
      }}
      title={username}
    >
      {initials}
    </div>
  );
}

export function CommitAvatar({username}: {username: string}) {
  const url = useAtomValue(avatarUrl(username));

  if (url) {
    return <AvatarImg url={url} username={username} />;
  }

  return <InitialsAvatar username={username} />;
}
```

### Selection Highlighting CSS
```css
/* Source: PRDashboard.css lines 171-178 (verified ISL pattern)
 * VS Code selection pattern from https://github.com/microsoft/vscode/issues/45479
 */

.commit-row-selected {
  /* Existing background color */
  background-color: var(--selected-commit-background, rgba(74, 144, 226, 0.15));

  /* Add VS Code-style left border accent */
  border-left: 3px solid var(--graphite-accent, #4a90e2);
  margin-left: -3px; /* Prevent layout shift */

  /* Subtle transition for smooth appearance */
  transition: background-color 0.1s ease, border-left-color 0.1s ease;
}

/* Subtle hover state before selecting */
.commit-rows:hover:not(.commit-row-selected) {
  background-color: rgba(255, 255, 255, 0.03);
  cursor: pointer;
}

/* Optional: Enhance selection with gradient like origin/main */
.commit-row-selected .commit-details {
  background: linear-gradient(
    90deg,
    rgba(74, 144, 226, 0.08) 0%,
    transparent 50%
  );
}
```

### Bidirectional Selection Sync Pattern
```typescript
// Source: Existing pattern from selection.ts useCommitCallbacks
// Shows how selection already works bidirectionally

// In selection.ts - already handles selection from commit tree
export function useCommitCallbacks(commit: CommitInfo) {
  const {isSelected, onClickToSelect} = useCommitSelection(commit.hash);
  // onClick already updates selectedCommits atom
  return {isSelected, onClickToSelect};
}

// In CommitTreeList.tsx - already passes selection to rows
function useExtraCommitRowProps(info: DagCommitInfo) {
  const {isSelected, onClickToSelect} = useCommitCallbacks(info);
  return {
    onClick: onClickToSelect,
    className: isSelected ? 'commit-row-selected' : '',
  };
}

// Pattern: Auto-scroll hook watches the same selectedCommits atom
// No additional sync needed - Jotai atom is single source of truth
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| scrollIntoView with instant | scrollIntoView with behavior: 'smooth' | HTML5 spec | Better UX, respects prefers-reduced-motion |
| Random avatar colors | Hash-based deterministic colors | Community best practice ~2020 | Consistency, no backend storage |
| setTimeout for DOM updates | setTimeout vs requestAnimationFrame debate | Ongoing | setTimeout 100ms still standard for one-off scrolls |
| Inline styles | CSS variables + StyleX | ISL adopted StyleX | Theme-aware, maintainable |

**Deprecated/outdated:**
- `ReactDOM.findDOMNode()`: Deprecated in React 18, use refs instead
- `scrollIntoViewIfNeeded()`: Non-standard, use `scrollIntoView` with `block: 'nearest'`
- String refs: Deprecated, use `useRef` hook
- Random Math.random() colors for avatars: Non-deterministic, use hash function

**Current best practices (2026):**
- `scrollIntoView({behavior: 'smooth', block: 'center'})` is standard
- Hash-based avatar colors from username
- CSS variables for theme-aware colors
- Jotai atoms for state management (ISL's chosen approach)

## Open Questions

Things that couldn't be fully resolved:

1. **Avatar size: 20px or 24px?**
   - What we know: CONTEXT.md specifies 20-24px range
   - What's unclear: Exact size for visual balance with commit text
   - Recommendation: Start with 20px (smaller, less dominant), A/B test if needed

2. **Scroll on initial page load to HEAD?**
   - What we know: CONTEXT.md says "on initial page load, auto-scroll to current HEAD"
   - What's unclear: May be jarring if user expects to see top of history
   - Recommendation: Only scroll on initial load if HEAD is not in initial viewport. Add useRef to track first render vs subsequent selection changes

3. **Avatar position: left of title or in metadata row?**
   - What we know: Marked as "Claude's Discretion" in CONTEXT.md
   - What's unclear: Visual hierarchy preference
   - Recommendation: Left of title (inline with commit content) - makes author more prominent and follows GitHub PR list pattern

4. **Scroll animation duration?**
   - What we know: CONTEXT.md marks exact duration as discretion
   - What's unclear: Browser default vs custom timing
   - Recommendation: Use browser default (behavior: 'smooth' without custom duration) - respects user's prefers-reduced-motion setting

5. **Hover state opacity/color?**
   - What we know: Should show clickability, marked as discretion
   - What's unclear: Exact values
   - Recommendation: `rgba(255, 255, 255, 0.03)` - very subtle, matches ISL's minimal hover style

## Sources

### Primary (HIGH confidence)
- ISL Codebase Analysis:
  - `/Users/jonas/code/sapling/addons/isl/src/Avatar.tsx` - Existing avatar infrastructure
  - `/Users/jonas/code/sapling/addons/isl/src/ComparisonView/ComparisonView.tsx` (lines 110-125) - Verified scrollIntoView pattern
  - `/Users/jonas/code/sapling/addons/isl/src/selection.ts` - Selection state management
  - `/Users/jonas/code/sapling/addons/isl/src/RenderDag.tsx` (line 396) - data-commit-hash attribute
  - `/Users/jonas/code/sapling/addons/isl/src/CommitTreeList.css` - Existing CSS variables
  - `/Users/jonas/code/sapling/addons/isl/src/PRDashboard.css` (lines 171-178) - Selection border pattern

### Secondary (MEDIUM confidence)
- [React Scroll to Element: Master useRef & scrollIntoView](https://openillumi.com/en/en-react-useref-scrollintoview-control/) - React 18 scrollIntoView patterns
- [Scrolling a React Element into View](https://carlrippon.com/scrolling-a-react-element-into-view/) - useEffect timing
- [How to scroll to an element after render in React](https://www.codemzy.com/blog/react-scroll-to-element-on-render) - Custom hooks pattern
- [Smooth Scrolling with scrollIntoView in React](https://blog.saeloun.com/2023/06/08/scrolling-to-the-element-with-fixed-header-using-scrollintoview/) - behavior: 'smooth' with block: 'center'
- [Auto generate unique Avatar colors randomly](https://dev.to/admitkard/auto-generate-avatar-colors-randomly-138j) - Hash-based color generation
- [Creating Avatars With Colors Using The Modulus](https://marcoslooten.com/blog/creating-avatars-with-colors-using-the-modulus/) - Color palette selection algorithm
- [Deterministic React Avatar Fallbacks](https://www.joshuaslate.com/blog/deterministic-react-avatar-fallback) - Consistent color approach
- [VSCode Selection Border Issues](https://github.com/microsoft/vscode/issues/45479) - VS Code selection styling patterns
- [requestAnimationFrame vs setTimeout: When to Use Each](https://blog.openreplay.com/requestanimationframe-settimeout-use/) - Scroll timing comparison
- [WordPress Gutenberg PR #44573](https://github.com/WordPress/gutenberg/pull/44573) - Real-world scroll timing decision (chose rAF for list scrolling)

### Tertiary (LOW confidence - community patterns)
- General React hooks patterns from Medium articles
- Stack Overflow discussions (not directly cited)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All infrastructure exists in ISL codebase
- Architecture patterns: HIGH - Verified with existing ISL code (ComparisonView, PRDashboard)
- Auto-scroll timing: MEDIUM - setTimeout 100ms is proven in ISL but rAF debate ongoing
- Avatar hash colors: MEDIUM - Multiple sources agree on approach, not ISL-verified yet
- Pitfalls: HIGH - Based on common React patterns and ISL code analysis

**Research date:** 2026-01-22
**Valid until:** 30 days (stable patterns - React 18, CSS, scrollIntoView API are mature)

**Key validation needed during implementation:**
- Test scroll timing on slower devices (100ms may need adjustment)
- Verify avatar color palette has sufficient contrast for accessibility
- A/B test avatar size (20px vs 24px) for visual balance
- Confirm scroll-on-initial-load behavior feels natural to users
