# Roadmap: Sapling ISL Fork

## Milestones

- ✅ **v1.0 MVP** - Phases 1-5 (shipped 2026-01-22)
- 🚧 **v1.1 Refinement & Fixes** - Phases 6-8 (in progress)

## Overview

v1.1 addresses issues discovered during v1.0 usage and refines the color scheme to match Graphite more closely. Phase 6 fixes broken auto-scroll sync, Phase 7 polishes UI interactions and adds editor configuration, and Phase 8 refines the color palette for a more professional appearance.

## Phases

<details>
<summary>✅ v1.0 MVP (Phases 1-5) - SHIPPED 2026-01-22</summary>

### Phase 1: Layout Foundation
**Goal**: Three-column layout with responsive behavior and proper spacing
**Plans**: 3 plans

Plans:
- [x] 01-01: Base layout structure
- [x] 01-02: Responsive collapse behavior
- [x] 01-03: Spacing and padding refinement

### Phase 2: Stack Column
**Goal**: Click-to-checkout navigation and origin/main prominence
**Plans**: 3 plans

Plans:
- [x] 02-01: Click-to-checkout functionality
- [x] 02-02: Origin/main visual prominence
- [x] 02-03: Main branch button at top

### Phase 3: Commit Tree
**Goal**: Author avatars and auto-scroll sync
**Plans**: 2 plans

Plans:
- [x] 03-01: Author avatars with deterministic colors
- [x] 03-02: Auto-scroll sync with selection borders

### Phase 4: Details Panel
**Goal**: De-emphasize amend section, promote files changed
**Plans**: 2 plans

Plans:
- [x] 04-01: Collapsible amend section
- [x] 04-02: Promote files changed section

### Phase 5: Diff Polish
**Goal**: Graphite-style diff colors
**Plans**: 1 plan

Plans:
- [x] 05-01: Soft cyan-blue additions, salmon deletions

</details>

### 🚧 v1.1 Refinement & Fixes (In Progress)

**Milestone Goal:** Fix v1.0 issues discovered in usage and refine color scheme to match Graphite more closely.

#### Phase 6: Navigation Fixes
**Goal**: Auto-scroll sync properly positions selected commit and provides smooth navigation experience
**Depends on**: Phase 5 (v1.0 complete)
**Requirements**: NAV-01, NAV-02, NAV-03
**Success Criteria** (what must be TRUE):
  1. When user clicks commit in left column, middle column scrolls to show "you are here" commit at viewport top
  2. Auto-scroll works regardless of current scroll position in either column
  3. Scroll animation is smooth and doesn't cause jarring jumps
**Plans**: 2 plans

Plans:
- [x] 06-01-PLAN.md — Fix scroll alignment (block: start) and add CSS scroll-margin-top
- [ ] 06-02-PLAN.md — Gap closure: move scroll-margin-top to correct CSS element

#### Phase 7: UI Polish & Configuration
**Goal**: Reduce visual clutter, fix scroll issues, add line counts, and enable editor configuration
**Depends on**: Phase 5 (v1.0 complete, independent of Phase 6)
**Requirements**: UI-01, UI-02, UI-03, UI-04, UI-05, CFG-01, CFG-02, CFG-03, CFG-04
**Success Criteria** (what must be TRUE):
  1. Top action bar has reduced opacity by default and returns to full opacity on hover
  2. Left column has single scrollable area without nested scroll issues
  3. Files changed section shows line counts in "+123/-45" format with accurate statistics
  4. User can configure preferred editor path in ISL settings
  5. "Open files" button uses configured editor and shows clear error if path is invalid
  6. Editor configuration persists across sessions
**Plans**: TBD

Plans:
- [ ] 07-01: TBD

#### Phase 8: Design Refinement
**Goal**: Color scheme matches Graphite's darker, more muted palette for professional appearance
**Depends on**: Phase 7 (best to see refined behavior with refined colors)
**Requirements**: DES-01, DES-02, DES-03, DES-04, DES-05
**Success Criteria** (what must be TRUE):
  1. Background colors match Graphite's darker palette from reference screenshots
  2. Addition green is muted forest green with reduced saturation
  3. Gray hierarchy for file lists and UI elements matches Graphite
  4. Diff view backgrounds and text colors match Graphite screenshots
  5. Overall color saturation is reduced for professional, easy-on-eyes appearance
**Plans**: TBD

Plans:
- [ ] 08-01: TBD

## Progress

**Execution Order:** Phases execute in numeric order: 6 → 7 → 8

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Layout Foundation | v1.0 | 3/3 | Complete | 2026-01-22 |
| 2. Stack Column | v1.0 | 3/3 | Complete | 2026-01-22 |
| 3. Commit Tree | v1.0 | 2/2 | Complete | 2026-01-22 |
| 4. Details Panel | v1.0 | 2/2 | Complete | 2026-01-22 |
| 5. Diff Polish | v1.0 | 1/1 | Complete | 2026-01-22 |
| 6. Navigation Fixes | v1.1 | 1/2 | Gap closure | - |
| 7. UI Polish & Configuration | v1.1 | 0/? | Not started | - |
| 8. Design Refinement | v1.1 | 0/? | Not started | - |
