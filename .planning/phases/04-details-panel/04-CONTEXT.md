# Phase 4: Details Panel - Context

**Gathered:** 2026-01-22
**Status:** Ready for planning

<domain>
## Phase Boundary

Reorganize the details panel to make file changes prominent while de-emphasizing the amend section. Line counts should display in GitHub/Graphite style (+X/-Y). The panel hierarchy shifts focus to what changed in the commit, with amend controls accessible but not dominating.

</domain>

<decisions>
## Implementation Decisions

### File list presentation
- Group files by directory with collapsible folder structure
- All directories expanded by default — see all files immediately
- Change type indicated by filename color only (no badges or icons)
- Directory tree structure mirrors actual file paths

### Line count styling
- Right-aligned in each file row (GitHub style positioning)
- Format: `+123/-45` (compact with slash separator)
- Colors: Green for additions, red for deletions (standard git coloring)
- Slightly subdued prominence — smaller or lighter than filename, present but not competing

### Amend de-emphasis
- Collapsed by default as an accordion
- Collapsed header shows: "Changes to amend (X files)" with file count
- Always starts collapsed when selecting a different commit (no persistence)
- Expand chevron for user interaction

### Claude's Discretion
- Filename colors for change types (matching Graphite color scheme)
- Visual treatment for collapsed amend header (secondary styling)
- Exact font sizes and spacing for subdued line counts
- Directory collapse/expand animation

</decisions>

<specifics>
## Specific Ideas

- Line counts should feel like GitHub's PR file list — clearly there but filename is primary
- Amend section should feel like an "advanced" or "secondary" action area
- Directory grouping like VS Code's file explorer in source control view

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 04-details-panel*
*Context gathered: 2026-01-22*
