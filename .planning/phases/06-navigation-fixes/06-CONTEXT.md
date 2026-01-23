# Phase 6: Navigation Fixes - Context

**Gathered:** 2026-01-23
**Status:** Ready for planning

<domain>
## Phase Boundary

Fix auto-scroll sync between left column (commit list) and middle column (commit tree). When user clicks a commit in the left column, the middle column should scroll to properly position the selected "you are here" commit in the viewport. This addresses broken auto-scroll behavior from v1.0.

</domain>

<decisions>
## Implementation Decisions

### Scroll positioning strategy
- Target commit appears at **top of viewport with small padding** (20-40px above)
- **Always reposition** to this standard location, even if commit is already visible
- Provides consistent, predictable scroll behavior on every click

### Claude's Discretion
- Animation duration based on scroll distance (instant vs smooth scroll threshold)
- Exact padding value within 20-40px range
- Easing curve for scroll animation
- Edge case handling (target near top/bottom of list, rapid clicks)
- Coordination between manual scroll and click-triggered scroll

</decisions>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches for smooth scrolling and edge cases.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 06-navigation-fixes*
*Context gathered: 2026-01-23*
