# Phase 3: Commit Tree - Context

**Gathered:** 2026-01-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Synchronized selection and display improvements in the commit tree (middle column). Users see auto-scroll sync when selecting from stack, author identity on commits, and prominent origin/main highlighting. Changing stack column behavior or navigation patterns are separate phases.

</domain>

<decisions>
## Implementation Decisions

### Auto-scroll behavior
- Smooth scroll animation that centers the selected commit in view
- Scroll triggers immediately on selection (no delay)
- Selecting anywhere (stack or tree) centers the commit — consistent behavior
- On initial page load, auto-scroll to current HEAD commit

### Author display
- Small avatar circles (20-24px) — compact, doesn't dominate the row
- Username only (not full name) — familiar for team context
- Fallback for missing avatars: initials in colored circle, consistent color per user

### Selection visual feedback
- Left border accent (like VS Code) — colored bar on left edge
- Use Phase 1 accent color (soft blue #4a90e2) for consistency
- Subtle hover state on commits before selecting — shows clickability
- Bidirectional sync: selecting in tree highlights in stack, and vice versa

### Claude's Discretion
- Avatar position relative to commit info (left of title vs metadata row)
- Exact scroll animation duration
- Hover state opacity/color
- How to generate consistent colors for initials fallback

</decisions>

<specifics>
## Specific Ideas

- "Like VS Code" — left border accent for selection highlight
- Smooth scroll should feel polished, not jarring
- Selection sync should work both directions — feel unified

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 03-commit-tree*
*Context gathered: 2026-01-22*
