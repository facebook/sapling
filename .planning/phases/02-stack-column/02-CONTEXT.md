# Phase 2: Stack Column - Context

**Gathered:** 2025-01-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Users can navigate the PR stack by clicking commits/PRs to checkout. Main branch appears prominently at top with pull/checkout capability. Origin/main is visually distinct. This phase covers stack column interactivity and visual hierarchy — not commit tree sync or details panel changes.

</domain>

<decisions>
## Implementation Decisions

### Click-to-checkout behavior
- Single click triggers checkout (not double-click)
- Entire stack item row is clickable
- Clicking a commit inside an expanded PR: pulls the PR and checks out, updating working copy to match that commit (same behavior as "go to" button in middle column)
- Subtle visual feedback during checkout (dims or pulses briefly)
- Hover state: pointer cursor + subtle highlight on clickable items
- Currently checked-out item: subtle indicator (not bold/strong)
- Re-clicking already checked-out item: no-op

### Main branch presentation
- Fixed at top of stack column (always visible, items scroll beneath)
- Single "Go to main" action (pulls and checks out in one click)
- Shows sync status (e.g., "behind by X commits" or "up to date")
- Subtle visual distinction from PR stack items (not dramatically different)

### Origin/main distinction
- Appears in both places: main section at top AND marker in commit history
- Moderate prominence in commit history (noticeable but doesn't dominate)
- Clicking origin/main in history: same behavior as other commits (checkout)

### Stack item layout
- Full details on each PR: number, title, status, author, time since update
- Comfortable density (moderate spacing, balance of density and readability)
- Keep current ISL expansion behavior for showing commits within PRs

### Claude's Discretion
- Exact visual treatment for origin/main marker (color, icon, or badge)
- Error handling appearance for failed checkouts (follow existing ISL patterns)
- Visual indicator style for stacked PR relationships
- Exact hover highlight colors (within Graphite color scheme)
- Loading feedback implementation details

</decisions>

<specifics>
## Specific Ideas

- Click behavior should mirror the existing "go to" button functionality in the middle column — clicking a commit in the stack does the same thing
- Main branch fixed at top similar to how Slack keeps workspace switcher fixed
- Sync status on main helps users know at a glance if they need to pull

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-stack-column*
*Context gathered: 2025-01-22*
