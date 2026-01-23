# Requirements: Sapling ISL Fork

**Defined:** 2026-01-23
**Core Value:** The UI should feel polished and effortless — you focus on the code, not fighting the interface.

## v1.1 Requirements

Requirements for v1.1 refinement and fixes milestone.

### Navigation

- [ ] **NAV-01**: When user clicks commit in left column, middle column scrolls to show "you are here" commit at viewport top
- [ ] **NAV-02**: Auto-scroll behavior works regardless of current scroll position in either column
- [ ] **NAV-03**: Scroll animation is smooth and doesn't jar the user

### UI Polish

- [ ] **UI-01**: Top action bar in middle column has reduced opacity (e.g., 0.7) by default
- [ ] **UI-02**: Top action bar returns to full opacity on hover
- [ ] **UI-03**: Left column has single scrollable area (no nested scroll on "PR Stacks" divider)
- [ ] **UI-04**: Files changed section shows line counts as "+123/-45" format
- [ ] **UI-05**: Line count statistics accurately reflect additions and deletions

### Design

- [ ] **DES-01**: Background colors match Graphite's darker palette from screenshots
- [ ] **DES-02**: Addition green is muted/forest green (less saturated than v1.0)
- [ ] **DES-03**: Gray hierarchy for file lists and UI elements matches Graphite
- [ ] **DES-04**: Diff view backgrounds and text colors match Graphite screenshots
- [ ] **DES-05**: Overall color saturation is reduced for professional appearance

### Configuration

- [ ] **CFG-01**: User can configure preferred editor path in ISL settings
- [ ] **CFG-02**: "Open files" button uses configured editor
- [ ] **CFG-03**: Editor configuration persists across sessions
- [ ] **CFG-04**: Clear error message if configured editor path is invalid

## Out of Scope

| Feature | Reason |
|---------|--------|
| Mobile responsiveness improvements | Desktop/laptop focus, no mobile users |
| New stack management features | v1.1 is refinement only, defer new capabilities |
| Upstream contribution | Internal fork, not submitting changes upstream |

## Traceability

Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| NAV-01 | Phase 6 | Pending |
| NAV-02 | Phase 6 | Pending |
| NAV-03 | Phase 6 | Pending |
| UI-01 | Phase 7 | Pending |
| UI-02 | Phase 7 | Pending |
| UI-03 | Phase 7 | Pending |
| UI-04 | Phase 7 | Pending |
| UI-05 | Phase 7 | Pending |
| DES-01 | Phase 8 | Pending |
| DES-02 | Phase 8 | Pending |
| DES-03 | Phase 8 | Pending |
| DES-04 | Phase 8 | Pending |
| DES-05 | Phase 8 | Pending |
| CFG-01 | Phase 7 | Pending |
| CFG-02 | Phase 7 | Pending |
| CFG-03 | Phase 7 | Pending |
| CFG-04 | Phase 7 | Pending |

**Coverage:**
- v1.1 requirements: 17 total
- Mapped to phases: 17 ✓
- Unmapped: 0 ✓

---
*Requirements defined: 2026-01-23*
*Last updated: 2026-01-23 after roadmap creation*
