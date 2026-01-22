# Phase 1: Layout & Foundation - Context

**Gathered:** 2025-01-21
**Status:** Ready for planning

<domain>
## Phase Boundary

Responsive three-column layout with proper spacing and Graphite-inspired colors. Users see breathing room, graceful collapse on narrow windows, and a navy/muted color scheme. This phase establishes the visual foundation that all subsequent phases build upon.

</domain>

<decisions>
## Implementation Decisions

### Collapse behavior
- Hide columns progressively: details panel hides first, then stack column
- First breakpoint at 1200px — details panel hides below this width
- Hidden columns accessible via same click mechanism as current manual hide/show
- Smart restore behavior: if user manually hid a column, it stays hidden; if auto-hidden due to width, it auto-restores when window widens

### Color palette
- Deep navy background (#1a1f36 range) — Graphite-style, professional, easy on eyes
- All three columns share the same background — clean, unified look
- Soft blue accent color for interactive elements (buttons, selected items, links)
- Dark-only theme for now — light mode is future work if needed
- Subtle borders between columns/sections — thin lines for clear separation
- Subtle glow effect on hover states

### Visual hierarchy
- Middle column (commit tree) is the most prominent
- Prominence achieved through combination: more width, visual elevation, and more breathing room
- Subtle section headers — present but understated, content takes focus
- Subtle highlight for selected/active items — slight background change, visible but not loud

### Claude's Discretion
- Text colors (primary vs secondary) — optimize for readability on navy
- Border colors — pick what works with the navy background
- Exact spacing values and padding amounts
- Exact breakpoint for second collapse (stack column)
- Shadow/glow intensity values

</decisions>

<specifics>
## Specific Ideas

- "Graphite-style" is the reference — deep navy, soft blues, muted tones, professional feel
- Middle column should feel like the star, side columns support it
- Borders should be "there but not distracting"

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 01-layout-foundation*
*Context gathered: 2025-01-21*
