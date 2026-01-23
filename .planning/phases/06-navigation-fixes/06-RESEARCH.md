# Phase 6: Navigation Fixes - Research

**Researched:** 2026-01-23
**Domain:** React smooth scroll synchronization between columns using native scrollIntoView API
**Confidence:** HIGH

## Summary

This phase implements auto-scroll synchronization between the left column (PR Dashboard) and middle column (CommitTreeList) when users click commits. The standard approach is using the native `scrollIntoView()` API with CSS `scroll-padding-top` for offset control.

The existing codebase already has a partial implementation in `CommitTreeList.tsx` (lines 254-276) using `scrollIntoView()` with smooth behavior and center alignment. The fix needs to change alignment from `block: 'center'` to `block: 'start'`, add proper padding, handle edge cases, and trigger from both column click events.

Key technical challenges include timing (ensuring DOM is ready), handling rapid clicks (needs throttling), and coordinating between manual scroll and programmatic scroll to avoid conflicts.

**Primary recommendation:** Use native `scrollIntoView()` with `block: 'start'` alignment, CSS `scroll-padding-top` for offset control, debounced click handlers for rapid clicks, and setTimeout(0) wrapper for DOM timing.

## Standard Stack

The established approach for this domain:

### Core
| Library/API | Version | Purpose | Why Standard |
|-------------|---------|---------|--------------|
| scrollIntoView() | Native Web API | Programmatic scrolling | Native, no dependencies, widely supported since 2020 |
| scroll-padding-top | CSS Standard | Offset for fixed headers | Native CSS property, recommended over JS calculations |
| useRef + useEffect | React 18.3.1 | DOM access + lifecycle | React standard pattern for DOM manipulation |
| setTimeout | Native | DOM render timing | Ensures DOM painted before scroll |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| debounce (shared/debounce.ts) | In-repo | Rate limiting | Already in codebase for rapid click handling |
| useThrottledEffect | In-repo | Throttled effects | For analytics/logging, not core scroll logic |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| scrollIntoView() | react-scroll library | Adds dependency (not in package.json), native API sufficient |
| scrollIntoView() | Manual scrollTo + math | More code, harder to maintain, native handles edge cases |
| CSS scroll-padding | JS offset calculation | Complex, error-prone, CSS is declarative and simpler |

**Installation:**
No new dependencies needed. All tools are native browser APIs or already in the codebase.

## Architecture Patterns

### Recommended Implementation Structure
```
CommitTreeList.tsx
├── useScrollToSelectedCommit() hook (exists, needs modification)
│   ├── Watch selectedCommits state
│   ├── setTimeout(0) for DOM timing
│   └── scrollIntoView with block: 'start'
└── CSS: .commit-row { scroll-margin-top: 30px; }

PRDashboard.tsx
├── handleStackCheckout() (exists)
│   ├── Update selected commit
│   └── Let CommitTreeList useEffect handle scroll
└── Individual PR rows: onClick triggers selection

selection.ts
└── useCommitCallbacks() (exists)
    └── onClickToSelect: triggers selection change
```

### Pattern 1: Native scrollIntoView with CSS Offset
**What:** Use browser-native scrollIntoView API with CSS scroll-padding-top for offset control.

**When to use:** When you need smooth scrolling with fixed header compensation.

**Example:**
```typescript
// Source: https://developer.mozilla.org/en-US/docs/Web/API/Element/scrollIntoView
const element = document.querySelector(`[data-commit-hash="${hash}"]`);
if (element) {
  element.scrollIntoView({
    behavior: 'smooth',
    block: 'start',      // Align to top, not center
    inline: 'nearest'
  });
}
```

**CSS Offset:**
```css
/* Source: https://developer.mozilla.org/en-US/docs/Web/CSS/scroll-padding-top */
.commit-row {
  scroll-margin-top: 30px; /* Applied to target element */
}
/* OR */
.commit-tree-root {
  scroll-padding-top: 30px; /* Applied to scroll container */
}
```

### Pattern 2: setTimeout(0) for DOM Timing
**What:** Wrap scrollIntoView in setTimeout(0) to ensure DOM has painted after state updates.

**When to use:** When scrollIntoView runs before React finishes rendering new elements.

**Example:**
```typescript
// Source: https://felixgerschau.com/react-hooks-settimeout/
useEffect(() => {
  if (selected.size !== 1) return;

  const hash = Array.from(selected)[0];
  const timer = setTimeout(() => {
    const element = document.querySelector(`[data-commit-hash="${hash}"]`);
    element?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, 0); // Defer until after browser paint

  return () => clearTimeout(timer); // Cleanup on unmount
}, [selected]);
```

### Pattern 3: Debounced Click Handlers for Rapid Clicks
**What:** Use debounce with leading=true to handle rapid clicks without scroll conflicts.

**When to use:** When users might click multiple commits quickly.

**Example:**
```typescript
// Source: addons/shared/debounce.ts (in-repo)
import {debounce} from 'shared/debounce';

const debouncedScroll = useCallback(
  debounce(
    (hash: string) => {
      const element = document.querySelector(`[data-commit-hash="${hash}"]`);
      element?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    },
    100,  // 100ms throttle
    undefined,
    true  // leading: execute first call immediately
  ),
  []
);
```

### Anti-Patterns to Avoid
- **Manual scrollTo calculations:** Don't calculate scroll positions manually. scrollIntoView handles viewport math, nested scroll containers, and edge cases automatically.
- **Using block: 'center' for top alignment:** This positions element in viewport center, not at top. Use `block: 'start'` for top alignment.
- **No setTimeout wrapper:** Calling scrollIntoView immediately in useEffect can fail if DOM hasn't rendered. Always wrap in setTimeout(0).
- **Ignoring rapid clicks:** Multiple quick clicks cause scroll conflicts. Use debounce with leading=true.

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Scroll offset for fixed headers | JS math to calculate scroll position minus header height | CSS `scroll-padding-top` or `scroll-margin-top` | Browser handles edge cases (zoom, nested scrollers, RTL). CSS is declarative. |
| Smooth scroll animation | Custom requestAnimationFrame loop with easing functions | Native `scrollIntoView({ behavior: 'smooth' })` | Browser optimizes for performance, respects user preferences (prefers-reduced-motion), handles interruption by user scroll. |
| Timing scroll after render | Multiple setTimeout attempts or requestAnimationFrame loops | Single `setTimeout(callback, 0)` | Defers until after paint. More attempts add complexity without benefit. |
| Rate limiting clicks | Custom throttle/debounce implementation | `shared/debounce.ts` (already in repo) | Already tested, handles edge cases, has dispose/reset methods. |
| Detecting scroll container | Walk DOM tree to find scrollable parent | Let scrollIntoView do it | Native API handles nested scroll containers, shadow DOM, and edge cases automatically. |

**Key insight:** The browser's native scrollIntoView is highly optimized and battle-tested. Custom implementations must handle: nested scroll containers, zoom levels, RTL languages, prefers-reduced-motion, user scroll interruption, and viewport edge cases. The native API does this already.

## Common Pitfalls

### Pitfall 1: Element Not Yet in DOM
**What goes wrong:** scrollIntoView called before React finishes rendering, element not found.

**Why it happens:** useEffect runs synchronously after render, but browser hasn't painted yet. If element is newly added or conditionally rendered, querySelector returns null.

**How to avoid:**
```typescript
const timer = setTimeout(() => {
  element?.scrollIntoView({ behavior: 'smooth', block: 'start' });
}, 0); // Defer until after browser paint
```

**Warning signs:** Scroll works sometimes but not consistently. Works on slow renders but fails on fast ones.

### Pitfall 2: Scroll Conflict with Manual User Scroll
**What goes wrong:** User manually scrolls while programmatic scroll is animating. Scroll stutters, jumps back, or zig-zags.

**Why it happens:** Native smooth scroll is an animation. User scroll events don't automatically cancel programmatic scroll. Both try to control scroll position simultaneously.

**How to avoid:** Native scrollIntoView actually handles this well in modern browsers - user scroll interrupts the animation automatically. For extra safety, track scroll state:

```typescript
const scrollingRef = useRef(false);

useEffect(() => {
  const handleUserScroll = () => {
    scrollingRef.current = false; // Cancel tracking if user scrolls
  };

  const container = document.querySelector('.commit-tree-root');
  container?.addEventListener('wheel', handleUserScroll, { passive: true });
  container?.addEventListener('touchstart', handleUserScroll, { passive: true });

  return () => {
    container?.removeEventListener('wheel', handleUserScroll);
    container?.removeEventListener('touchstart', handleUserScroll);
  };
}, []);
```

**Warning signs:** Scroll jitters when user touches scroll area during animation. Console errors about scroll conflicts.

### Pitfall 3: Rapid Clicks Causing Multiple Scrolls
**What goes wrong:** User clicks multiple commits quickly. Multiple scroll animations queue up. Scroll jumps between targets erratically.

**Why it happens:** Each click triggers a new useEffect run with new selected commit. Each effect calls scrollIntoView. Browser queues multiple scroll animations.

**How to avoid:** Use debounce with leading=true (execute first, ignore subsequent for delay period):

```typescript
const debouncedScroll = useCallback(
  debounce(scrollToHash, 100, undefined, true),
  []
);
```

**Warning signs:** Clicking rapidly causes scroll to "bounce" between commits. Last click doesn't always win.

### Pitfall 4: Wrong Alignment Block Value
**What goes wrong:** Using `block: 'center'` centers element in viewport. Requirement is "top of viewport with padding."

**Why it happens:** Existing code uses `block: 'center'` for centered positioning. Requirements changed to top positioning.

**How to avoid:** Change to `block: 'start'` and use CSS for padding:

```typescript
element.scrollIntoView({
  behavior: 'smooth',
  block: 'start',    // NOT 'center'
  inline: 'nearest'
});
```

**Warning signs:** Commit appears in middle of viewport instead of at top. User context changed - decision states "top of viewport with padding."

### Pitfall 5: Forgetting Cleanup in useEffect
**What goes wrong:** setTimeout continues after component unmounts. Tries to scroll element that no longer exists. Memory leak.

**Why it happens:** useEffect runs cleanup on unmount, but setTimeout keeps reference to stale DOM.

**How to avoid:** Always return cleanup function:

```typescript
useEffect(() => {
  const timer = setTimeout(() => { /* scroll */ }, 0);
  return () => clearTimeout(timer); // CRITICAL
}, [selected]);
```

**Warning signs:** Console warnings about setting state on unmounted component. Memory usage grows over time.

## Code Examples

Verified patterns from official sources:

### Basic scrollIntoView with Modern Options
```typescript
// Source: https://developer.mozilla.org/en-US/docs/Web/API/Element/scrollIntoView
const element = document.querySelector(`[data-commit-hash="${hash}"]`);
if (element) {
  element.scrollIntoView({
    behavior: 'smooth',  // Animate scroll
    block: 'start',      // Align top edge to viewport top
    inline: 'nearest'    // Minimal horizontal scroll
  });
}
```

### CSS Offset for Fixed Headers
```css
/* Source: https://developer.mozilla.org/en-US/docs/Web/CSS/scroll-padding-top */
/* Option 1: scroll-margin on target element */
.commit-row {
  scroll-margin-top: 30px;
}

/* Option 2: scroll-padding on container */
.commit-tree-root {
  scroll-padding-top: 30px;
}
```

### Complete React Hook with Timing and Cleanup
```typescript
// Source: https://aghilesgoumeziane.com/blog/effective-solutions-for-scrollintoview-problems-in-reacts-useeffect
function useScrollToSelectedCommit() {
  const selected = useAtomValue(selectedCommits);

  useEffect(() => {
    if (selected.size !== 1) {
      return;
    }

    const hash = Array.from(selected)[0];
    const timer = setTimeout(() => {
      const element = document.querySelector(`[data-commit-hash="${hash}"]`);
      if (element) {
        element.scrollIntoView({
          behavior: 'smooth',
          block: 'start',
          inline: 'nearest',
        });
      }
    }, 0); // Defer until after browser paint

    return () => clearTimeout(timer);
  }, [selected]);
}
```

### Debounced Scroll for Rapid Clicks
```typescript
// Source: addons/shared/debounce.ts
import {debounce} from 'shared/debounce';

const scrollToHash = (hash: string) => {
  const element = document.querySelector(`[data-commit-hash="${hash}"]`);
  element?.scrollIntoView({ behavior: 'smooth', block: 'start' });
};

const debouncedScroll = useCallback(
  debounce(scrollToHash, 100, undefined, true), // leading: true
  []
);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| scrollTo() with manual math | scrollIntoView() with CSS offset | Baseline support since Jan 2020 | Simpler, more reliable, respects user preferences |
| JS offset calculations | scroll-padding-top CSS | Widely supported 2019+ | Declarative, handles edge cases automatically |
| Custom scroll libraries | Native smooth scroll | Native support mature 2020+ | Remove dependencies, better performance |
| block: 'center' | block: 'start' + CSS margin | User decision (phase context) | Consistent positioning at top with padding |

**Deprecated/outdated:**
- **Manual scrollTo calculations:** Native scrollIntoView handles nested containers and edge cases better
- **react-scroll library for basic scrolling:** Native API is sufficient for this use case
- **Polyfills for scrollIntoView smooth behavior:** Baseline support now, no polyfill needed

## Open Questions

Things that couldn't be fully resolved:

1. **What exact padding value (20-40px range)?**
   - What we know: User decision allows 20-40px range. Existing CSS may have header/toolbar heights to reference.
   - What's unclear: Optimal value for visual balance. May depend on header height.
   - Recommendation: Start with 30px (mid-range), adjust based on existing header heights in layout. Check `.main-content-area` and `.commit-tree-root` CSS for context.

2. **Should scroll behavior change based on distance?**
   - What we know: User marked as "Claude's discretion" - instant vs smooth scroll threshold.
   - What's unclear: At what distance threshold should scroll be instant vs smooth?
   - Recommendation: Test with users. Large scroll distances (>2000px) might benefit from instant scroll to avoid long animations. Start with always-smooth, add distance check only if users report slow animations.

3. **How to coordinate with existing scroll state in PR Dashboard?**
   - What we know: PRDashboard has its own scroll container. Clicking commits there should scroll middle column.
   - What's unclear: Does PR Dashboard also need to scroll its own list to keep clicked item visible?
   - Recommendation: Test UX. If PR list is short, no action needed. If long, may need bidirectional scroll sync (left scrolls to show selected, middle scrolls to show commit).

## Sources

### Primary (HIGH confidence)
- [MDN: Element.scrollIntoView()](https://developer.mozilla.org/en-US/docs/Web/API/Element/scrollIntoView) - Official Web API documentation
- [MDN: scroll-padding-top](https://developer.mozilla.org/en-US/docs/Web/CSS/scroll-padding-top) - CSS property specification
- Codebase: `addons/shared/debounce.ts` - In-repo debounce utility
- Codebase: `addons/isl/src/CommitTreeList.tsx` - Existing scroll implementation (lines 254-276)

### Secondary (MEDIUM confidence)
- [Saeloun Blog: scrollIntoView with Fixed Header](https://blog.saeloun.com/2023/06/08/scrolling-to-the-element-with-fixed-header-using-scrollintoview/) - React pattern with CSS offset
- [Aghiles Goumeziane: scrollIntoView in useEffect](https://aghilesgoumeziane.com/blog/effective-solutions-for-scrollintoview-problems-in-reacts-useeffect) - setTimeout(0) pattern
- [Felix Gerschau: setTimeout in React Hooks](https://felixgerschau.com/react-hooks-settimeout/) - Cleanup pattern

### Tertiary (LOW confidence)
- [CSS-Tricks: Debouncing and Throttling](https://css-tricks.com/debouncing-throttling-explained-examples/) - General concepts, not React-specific
- [GitHub Issues: scroll conflicts](https://github.com/software-mansion/react-native-reanimated/issues/1699) - React Native patterns may not apply directly

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Native APIs, well-documented, already partially in codebase
- Architecture: HIGH - Clear pattern from existing implementation, minor modifications needed
- Pitfalls: MEDIUM - Timing/debounce issues common in community, solutions well-known but require testing

**Research date:** 2026-01-23
**Valid until:** 30 days (stable APIs, unlikely to change)
