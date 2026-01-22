# Phase 1: Layout & Foundation - Research

**Researched:** 2026-01-21
**Domain:** React responsive layout with StyleX, Flexbox, and ResizeObserver
**Confidence:** HIGH

## Summary

Phase 1 focuses on building a responsive three-column layout with Graphite-inspired styling in a React/TypeScript/Vite application. The codebase already uses StyleX 0.9.3 for styling, Jotai for state management, and has existing drawer components with collapse/expand functionality.

The standard approach for this phase involves:
1. **Responsive layout**: Use CSS Flexbox with ResizeObserver API to track viewport changes and trigger responsive behavior
2. **State management**: Leverage existing Jotai atoms for drawer state and add responsive breakpoint state
3. **Styling**: Extend existing StyleX theme tokens for Graphite-style navy colors and spacing system
4. **Collapse behavior**: Enhance existing `Drawers.tsx` component with auto-collapse based on width thresholds

**Primary recommendation:** Build on the existing drawer infrastructure (`Drawers.tsx`, `drawerState.ts`, `responsive.tsx`) rather than creating new layout components from scratch. Use ResizeObserver (already implemented in `useMainContentWidth`) for width detection, StyleX for theming, and Jotai atoms for state management.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| React | 18.3.1 | UI framework | Industry standard for component-based UIs, excellent TypeScript support |
| StyleX | 0.9.3 | Styling solution | Meta's atomic CSS compiler, already in use, supports theming and responsive design |
| Jotai | 2.6.2 | State management | Atomic state management, already in use, perfect for reactive UI state |
| TypeScript | 5.5.4 | Type safety | Type-safe development, prevents runtime errors |
| Vite | 5.4.12 | Build tool | Fast development server, already configured |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| ResizeObserver API | Native browser | Element size tracking | Already used in `responsive.tsx` for width detection |
| CSS Flexbox | Native CSS | Layout system | Three-column responsive layouts, already used in `Drawers.css` |
| CSS Custom Properties | Native CSS | Theme variables | Already used extensively for VS Code theme integration |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Flexbox | CSS Grid | Grid offers better 2D control but Flexbox is already used throughout codebase |
| ResizeObserver | window.resize events | ResizeObserver tracks specific elements, not just viewport, and is more performant |
| Jotai atoms | React Context | Context would work but Jotai is already the project standard and more performant |
| StyleX | CSS Modules/Styled Components | StyleX is already configured and provides atomic CSS benefits |

**Installation:**
No new dependencies needed - all required libraries are already installed.

## Architecture Patterns

### Recommended Project Structure
```
addons/isl/src/
├── Drawers.tsx              # Main three-column layout (already exists)
├── Drawers.css              # Layout styles (already exists)
├── drawerState.ts           # Jotai state for drawer collapse (already exists)
├── responsive.tsx           # Responsive utilities (already exists)
└── theme.tsx                # Theme state management (already exists)

addons/components/theme/
└── tokens.stylex.ts         # StyleX design tokens (already exists)
```

### Pattern 1: ResizeObserver for Width Detection
**What:** Track the width of specific DOM elements and update state accordingly
**When to use:** Auto-collapsing columns based on available width
**Example:**
```typescript
// Source: addons/isl/src/responsive.tsx (existing pattern)
export function useMainContentWidth() {
  const setMainContentWidth = useSetAtom(mainContentWidthState);
  const mainContentRef = useRef<null | HTMLDivElement>(null);

  useEffect(() => {
    const element = mainContentRef.current;
    if (element == null) return;

    const obs = new ResizeObserver(entries => {
      const [entry] = entries;
      setMainContentWidth(entry.contentRect.width);
    });
    obs.observe(element);
    return () => obs.unobserve(element);
  }, [mainContentRef, setMainContentWidth]);

  return mainContentRef;
}
```

### Pattern 2: Jotai Atoms for Responsive State
**What:** Derive responsive state from width measurements using computed atoms
**When to use:** Triggering layout changes at specific breakpoints
**Example:**
```typescript
// Source: addons/isl/src/responsive.tsx (existing pattern)
export const mainContentWidthState = atom(500);

export const isNarrowCommitTree = atom(
  get => get(mainContentWidthState) < NARROW_COMMIT_TREE_WIDTH
);
```

### Pattern 3: StyleX with Media Queries and Hover States
**What:** Define responsive styles with conditional values
**When to use:** Hover effects, breakpoint-specific styling
**Example:**
```typescript
// Source: StyleX documentation
import * as stylex from '@stylexjs/stylex';

const styles = stylex.create({
  button: {
    backgroundColor: {
      default: 'lightblue',
      ':hover': 'blue',
      '@media (hover: hover)': {
        ':hover': 'blue',
      },
    },
    width: {
      default: '300px',
      '@media (max-width: 1200px)': '100%',
    },
  },
});
```

### Pattern 4: StyleX Theme Variables
**What:** Define design tokens using `stylex.defineVars()` for consistent theming
**When to use:** Creating color palettes, spacing systems, and other design tokens
**Example:**
```typescript
// Source: addons/components/theme/tokens.stylex.ts (existing pattern)
export const colors = stylex.defineVars({
  bg: 'var(--background)',
  fg: 'var(--foreground)',
  hoverDarken: 'rgba(255, 255, 255, 0.1)',
  subtleHoverDarken: 'rgba(255, 255, 255, 0.03)',
});

export const spacing = stylex.defineVars({
  none: '0px',
  half: '5px',
  pad: '10px',
  double: '20px',
});
```

### Pattern 5: Flexbox Three-Column Layout with Wider Center
**What:** Use flex values to control relative column widths
**When to use:** Making the middle column more prominent
**Example:**
```css
/* Source: CSS Flexbox best practices */
.container {
  display: flex;
  flex-direction: row;
}
.col-left, .col-right {
  flex: 1; /* Takes 1 part of available space */
}
.col-middle {
  flex: 2; /* Takes 2 parts - twice as wide as sides */
}
```

### Anti-Patterns to Avoid
- **Fixed pixel widths for columns:** Use flex or percentage-based widths for responsive behavior
- **Too many breakpoints:** Limit to 2-3 essential breakpoints to avoid complexity
- **window.resize listeners without debounce:** Use ResizeObserver instead, it's more efficient
- **Hardcoded color values:** Use CSS custom properties or StyleX tokens for theme consistency
- **Pure white on pure black:** Use slightly off-white/off-black to reduce eye strain

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Drawer collapse/expand | Custom show/hide logic | Existing `Drawers.tsx` component | Already has resize handles, collapsed state, keyboard shortcuts |
| Element width tracking | Custom resize listener | ResizeObserver API (in `responsive.tsx`) | Native API, already implemented, more performant than window.resize |
| Theme state management | Custom theme context | Existing `theme.tsx` + StyleX tokens | Already handles VS Code theme integration, platform preferences |
| Responsive state | Multiple useState hooks | Jotai atoms with derived state | Already the project pattern, prevents prop drilling, auto-memoization |
| Spacing values | Magic numbers | StyleX `spacing` tokens | Consistency across codebase, easy to update globally |
| Debouncing | Custom debounce function | `shared/debounce` utility | Already exists in codebase, battle-tested |

**Key insight:** The ISL codebase already has sophisticated responsive infrastructure. New work should extend existing patterns rather than reinvent them. The `Drawers.tsx` component already handles three-column layout, collapse/expand, resizing, and state persistence.

## Common Pitfalls

### Pitfall 1: Ignoring Existing Drawer Infrastructure
**What goes wrong:** Building a new layout system from scratch when `Drawers.tsx` already exists
**Why it happens:** Not thoroughly exploring the codebase before starting implementation
**How to avoid:**
- Read `Drawers.tsx`, `drawerState.ts`, and `responsive.tsx` before writing any code
- Understand the existing drawer state atom structure (`{left: {size, collapsed}, right: {size, collapsed}}`)
- Recognize that manual collapse (user-triggered) vs auto-collapse (width-triggered) requires different state tracking
**Warning signs:** Creating new layout components, new state atoms, or new resize logic

### Pitfall 2: Device-Based Breakpoints Instead of Content-Based
**What goes wrong:** Setting breakpoints at common device widths (768px, 1024px) instead of where layout actually breaks
**Why it happens:** Following outdated responsive design tutorials from the mid-2010s
**How to avoid:**
- Test the layout at various widths and find where it looks cramped or awkward
- Use the user-specified breakpoints: 1200px for details panel (from CONTEXT.md)
- Choose the second breakpoint (stack column) based on actual layout needs, not device specs
**Warning signs:** Using 768px, 1024px, 1366px without testing if they actually make sense for this specific layout

### Pitfall 3: Not Distinguishing Manual vs Auto-Collapse
**What goes wrong:** When window widens, user's manually-collapsed drawer auto-expands (annoying UX)
**Why it happens:** Using a single boolean `collapsed` state without tracking collapse intent
**How to avoid:**
- Add separate state to track "was this auto-collapsed?" vs "user manually collapsed"
- On resize: auto-expand only if auto-collapsed, respect manual collapses
- Consider state structure: `{collapsed: boolean, autoCollapsed: boolean}` or `{collapsed: boolean, manuallyCollapsed: boolean}`
**Warning signs:** User complaints about "sticky" drawers or unexpected expansion behavior

### Pitfall 4: Glow Effects That Look Cheap
**What goes wrong:** Excessive blur radius or bright colors make hover states look amateurish
**Why it happens:** Copying examples without understanding subtlety requirements for professional UIs
**How to avoid:**
- Keep blur radius low (2-8px range) for subtle effects
- Use low opacity (10-20%) for glow colors
- Layer multiple shadows with increasing blur for depth: `box-shadow: 0 0 2px rgba(..., 0.3), 0 0 8px rgba(..., 0.1)`
- Test on actual navy background (#1a1f36) to ensure it looks professional
**Warning signs:** Glows visible from across the room, neon-like appearance, complaints about "distracting" UI

### Pitfall 5: Breaking VS Code Theme Integration
**What goes wrong:** Hardcoding colors breaks the existing VS Code theme system
**Why it happens:** Not understanding the existing CSS custom property architecture
**How to avoid:**
- Always use CSS custom properties from theme files: `var(--background)`, `var(--foreground)`, etc.
- Extend existing theme variables rather than replacing them
- Test with both VS Code dark and light themes (even though light mode is deferred)
- Check `addons/components/theme/themeDarkVariables.css` for available variables
**Warning signs:** Colors don't change when VS Code theme changes, hardcoded hex values in styles

### Pitfall 6: ResizeObserver Performance Issues
**What goes wrong:** ResizeObserver callback fires too frequently, causing performance degradation
**Why it happens:** Not understanding that ResizeObserver can fire multiple times per layout change
**How to avoid:**
- The existing implementation already handles this correctly (see `responsive.tsx`)
- Avoid heavy computation in ResizeObserver callbacks
- State updates trigger React re-renders automatically, no need for additional throttling with Jotai
- Don't create multiple ResizeObservers for the same element
**Warning signs:** UI lag during window resize, high CPU usage, janky animations

### Pitfall 7: StyleX Compilation Issues
**What goes wrong:** StyleX styles don't compile or generate incorrect CSS
**Why it happens:** Not following StyleX syntax requirements (default value required for conditional styles)
**How to avoid:**
- Always provide `default` when using conditional styles (`:hover`, `@media`)
- Use `null` as default value if no default style needed: `{default: null, ':hover': 'blue'}`
- Run `yarn build` to catch StyleX compilation errors early
- Follow existing patterns in codebase (see `Commit.tsx`, `stylexUtils.tsx`)
**Warning signs:** Build errors mentioning "default", styles not applying, missing CSS output

## Code Examples

Verified patterns from official sources:

### Example 1: Auto-Collapse Drawer Based on Width
```typescript
// Conceptual example based on existing patterns
import {atom, useAtom} from 'jotai';
import {islDrawerState} from './drawerState';

// Track whether collapse was auto-triggered (vs user-triggered)
const autoCollapsedState = atom({
  right: false,
  left: false,
});

// Breakpoint constants (from CONTEXT.md requirements)
const DETAILS_PANEL_BREAKPOINT = 1200;
const STACK_PANEL_BREAKPOINT = 800; // To be determined based on layout testing

// Derived atom that combines width and auto-collapse logic
const shouldAutoCollapse = atom(
  get => {
    const width = get(mainContentWidthState);
    return {
      right: width < DETAILS_PANEL_BREAKPOINT,
      left: width < STACK_PANEL_BREAKPOINT,
    };
  }
);

// Effect to auto-collapse/expand based on width
function useAutoCollapseDrawers() {
  const [drawerState, setDrawerState] = useAtom(islDrawerState);
  const [autoCollapsed, setAutoCollapsed] = useAtom(autoCollapsedState);
  const shouldCollapse = useAtomValue(shouldAutoCollapse);

  useEffect(() => {
    // Auto-collapse details panel if width too narrow
    if (shouldCollapse.right && !drawerState.right.collapsed) {
      setDrawerState(prev => ({
        ...prev,
        right: {...prev.right, collapsed: true}
      }));
      setAutoCollapsed(prev => ({...prev, right: true}));
    }

    // Auto-expand if width sufficient AND was auto-collapsed
    if (!shouldCollapse.right && drawerState.right.collapsed && autoCollapsed.right) {
      setDrawerState(prev => ({
        ...prev,
        right: {...prev.right, collapsed: false}
      }));
      setAutoCollapsed(prev => ({...prev, right: false}));
    }

    // Same logic for left drawer...
  }, [shouldCollapse, drawerState, autoCollapsed]);
}
```

### Example 2: StyleX Navy Color Theme Tokens
```typescript
// Extend addons/components/theme/tokens.stylex.ts
import * as stylex from '@stylexjs/stylex';

export const graphiteColors = stylex.defineVars({
  // Deep navy background (from CONTEXT.md)
  navyBg: '#1a1f36',

  // Soft blue accent for interactive elements
  blueAccent: {
    default: '#4a90e2',
    ':hover': '#5fa3f5',
  },

  // Text colors for readability on navy
  primaryText: '#e8eaed',
  secondaryText: '#9aa0a6',

  // Subtle borders
  borderColor: 'rgba(255, 255, 255, 0.1)',

  // Hover state subtle glow
  glowColor: 'rgba(74, 144, 226, 0.2)',
});

export const graphiteSpacing = stylex.defineVars({
  // Breathing room values
  compact: '8px',
  comfortable: '12px',
  spacious: '16px',
  extraSpacious: '24px',
});
```

### Example 3: Subtle Hover Glow Effect
```typescript
// StyleX hover effect with subtle glow
const styles = stylex.create({
  interactiveCard: {
    backgroundColor: {
      default: 'transparent',
      ':hover': 'rgba(255, 255, 255, 0.03)',
    },
    boxShadow: {
      default: 'none',
      ':hover': {
        default: null,
        '@media (hover: hover)': '0 0 4px rgba(74, 144, 226, 0.15), 0 0 12px rgba(74, 144, 226, 0.08)',
      },
    },
    transition: 'background-color 0.2s ease, box-shadow 0.2s ease',
    borderRadius: '4px',
    padding: '12px',
  },
});
```

### Example 4: Flexbox Three-Column with Wider Center
```typescript
// StyleX flexbox layout
const styles = stylex.create({
  threeColumnContainer: {
    display: 'flex',
    flexDirection: 'row',
    height: '100%',
    width: '100%',
  },

  leftColumn: {
    flex: '1 1 0',
    minWidth: '200px',
    maxWidth: '400px',
  },

  centerColumn: {
    flex: '2 1 0', // Twice the flex-grow of sides
    minWidth: '400px',
  },

  rightColumn: {
    flex: '1 1 0',
    minWidth: '250px',
    maxWidth: '450px',
  },
});
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| window.resize events | ResizeObserver API | ~2020 (browser support) | More performant, element-specific, no debouncing needed |
| Fixed device breakpoints | Content-based breakpoints | ~2023-2024 | More future-proof, adapts to actual content needs |
| CSS-in-JS with runtime | Atomic CSS (StyleX) | 2023 (StyleX release) | Zero runtime cost, smaller CSS bundle, better performance |
| Redux for all state | Specialized tools (Jotai, Zustand) | 2021-2024 | Less boilerplate, better performance, easier to learn |
| Pure white on black dark themes | Slightly muted colors | Ongoing | Better accessibility, reduced eye strain |

**Deprecated/outdated:**
- **window.matchMedia for width detection**: Use ResizeObserver for specific elements
- **Separate .css files for every component**: StyleX co-locates styles with components
- **Device-specific breakpoints**: Use content-driven breakpoints instead

## Open Questions

Things that couldn't be fully resolved:

1. **Second breakpoint value (stack column)**
   - What we know: First breakpoint at 1200px (from CONTEXT.md), general guidance is 800px range
   - What's unclear: Exact pixel value depends on actual column widths and content
   - Recommendation: Implement first breakpoint (1200px), then test to find natural breaking point for second collapse

2. **Shadow/glow intensity exact values**
   - What we know: Should be subtle, professional, layered shadows work best
   - What's unclear: Exact rgba values need to be tested against #1a1f36 background
   - Recommendation: Start with `0 0 4px rgba(74, 144, 226, 0.15), 0 0 12px rgba(74, 144, 226, 0.08)` and iterate

3. **StyleX vs CSS for drawer modifications**
   - What we know: Existing `Drawers.css` uses traditional CSS, codebase increasingly uses StyleX
   - What's unclear: Whether to migrate `Drawers.css` to StyleX or extend CSS file
   - Recommendation: Keep existing `Drawers.css` for now (working well), use StyleX for new theme tokens

4. **Integration with existing "render-compact" mode**
   - What we know: `responsive.tsx` has `renderCompactAtom` and `NARROW_COMMIT_TREE_WIDTH_WHEN_COMPACT`
   - What's unclear: How compact mode should interact with new auto-collapse behavior
   - Recommendation: Test both modes, ensure auto-collapse respects compact mode state

## Sources

### Primary (HIGH confidence)
- StyleX official documentation (stylexjs.com) - API usage, theming patterns
- ISL codebase (`Drawers.tsx`, `responsive.tsx`, `theme.tsx`, `tokens.stylex.ts`) - Existing patterns
- ResizeObserver MDN documentation - Browser API usage
- Jotai official documentation (jotai.org) - State management patterns

### Secondary (MEDIUM confidence)
- [Using the ResizeObserver API in React for responsive designs - LogRocket](https://blog.logrocket.com/using-resizeobserver-react-responsive-designs/)
- [useWindowSize Hook: Responsive React Apps Guide (2026)](https://react.wiki/hooks/window-size/)
- [Responsive Design Breakpoints: 2025 Playbook - DEV Community](https://dev.to/gerryleonugroho/responsive-design-breakpoints-2025-playbook-53ih)
- [CSS Flexbox Layout Guide - CSS-Tricks](https://css-tricks.com/snippets/css/a-guide-to-flexbox/)
- [3 Column Layouts (Responsive, Flexbox & CSS Grid)](https://matthewjamestaylor.com/3-column-layouts)
- [47 Best Glowing Effects in CSS [2026]](https://www.testmu.ai/blog/glowing-effects-in-css/)
- [Dark Mode with CSS: A Comprehensive Guide (2026)](https://618media.com/en/blog/dark-mode-with-css-a-comprehensive-guide/)

### Secondary (MEDIUM confidence) - Design Systems
- [Spacing units | U.S. Web Design System](https://designsystem.digital.gov/design-tokens/spacing-units/)
- [Spacing – Carbon Design System](https://v10.carbondesignsystem.com/guidelines/spacing/overview/)
- [77 Dark Blue And Navy Color Palettes | 2026](https://icolorpalette.com/dark-blue-and-navy)

### Tertiary (LOW confidence)
- WebSearch results on StyleX media queries - Limited official examples, community discussions helpful but not authoritative
- Color palette generators - Useful for inspiration but specific values need testing

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in use, versions verified in package.json
- Architecture: HIGH - Existing patterns identified in codebase, ResizeObserver is standard
- Pitfalls: MEDIUM - Based on general responsive design knowledge and codebase analysis, not ISL-specific documentation
- Color/glow values: LOW - Need actual testing against navy background to verify

**Research date:** 2026-01-21
**Valid until:** 30 days (stable domain, established patterns)
