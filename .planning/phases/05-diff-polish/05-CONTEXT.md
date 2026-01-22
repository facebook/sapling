# Phase 5: Diff Polish - Context

**Gathered:** 2026-01-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Transform diff colors from default harsh green/red to Graphite-style muted tones. Additions become soft blue, deletions become salmon/soft red. The goal is an easy-on-the-eyes diff view that matches the existing Graphite aesthetic established in earlier phases.

</domain>

<decisions>
## Implementation Decisions

### Color palette
- Additions: Soft cyan-blue — very desaturated, almost gray-blue tint (Graphite style)
- Deletions: Salmon pink — warm, peachy-red, very soft and muted
- Prominence: Moderate tint (~15-20% opacity) — clearly visible but not loud
- Gutter symbols (+/-): Slightly more saturated than line backgrounds to guide the eye

### Scope of styling
- Apply new colors to line backgrounds only
- Leave syntax highlighting colors unchanged
- Unchanged/context lines: No background (transparent, shows normal editor background)

### Claude's Discretion
- Inline word-level highlighting: Implement if already supported, skip if not
- Diff headers (file names, @@ markers): Adjust if needed for visual consistency
- Exact hex values that achieve the described aesthetic
- Gutter symbol saturation level

</decisions>

<specifics>
## Specific Ideas

- "Graphite style" is the north star — the existing Phase 1 colors (#1a1f36 navy, #4a90e2 accent) should inform the palette
- Soft, muted, easy on the eyes — the opposite of GitHub's harsh green/red

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-diff-polish*
*Context gathered: 2026-01-22*
