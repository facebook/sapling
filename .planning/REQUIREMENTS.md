# Requirements: Sapling ISL Fork

**Core Value:** The UI should feel polished and effortless — you focus on the code, not fighting the interface.

## v1.2 Requirements — PR Review View

Transform ISL into a complete PR review experience, matching GitHub's review workflow with Graphite's visual polish.

### Review Core

- [x] **REV-01**: User can enter review mode from PR row button
- [x] **REV-02**: User can see file list with navigation in review mode
- [x] **REV-03**: User can mark files as "viewed" with persistent checkmarks
- [x] **REV-04**: Viewed status resets when PR is updated (new commits)
- [x] **REV-05**: User can navigate file-by-file through diff view

### Comments

- [x] **COM-01**: User can add inline comments on specific diff lines
- [x] **COM-02**: User can add file-level comments (not tied to line)
- [x] **COM-03**: User can add PR-level general comments
- [x] **COM-04**: Comments are pending until review submission (batch workflow)
- [x] **COM-05**: User can see and reply to existing comment threads
- [x] **COM-06**: User can resolve/unresolve comment threads

### Review Submission

- [ ] **SUB-01**: User can submit review with "Approve" action
- [ ] **SUB-02**: User can submit review with "Request Changes" action
- [ ] **SUB-03**: User can submit review with "Comment" action (no approval decision)
- [ ] **SUB-04**: User can add summary text when submitting review

### Merge & CI

- [ ] **MRG-01**: User can see CI status before merging
- [ ] **MRG-02**: User can merge PR with strategy selection (merge, squash, rebase)
- [ ] **MRG-03**: Merge button disabled when CI failing or reviews pending

### Sync/Rebase

- [ ] **SYN-01**: User can sync current branch with latest main
- [ ] **SYN-02**: User can rebase all open PRs in stack on latest main
- [ ] **SYN-03**: User sees clear feedback during sync/rebase operation

### Stack Navigation

- [ ] **STK-01**: User can see stacked PR relationships in review mode
- [ ] **STK-02**: User can navigate between PRs in a stack without exiting review

## Future Requirements (v1.3+)

- Keyboard navigation (j/k for files, n/p for comments)
- Smart file filtering (hide generated, group by directory)
- Comment templates/suggestions
- Resume review experience (persist drafts across sessions)

## Out of Scope

| Feature | Reason |
|---------|--------|
| Auto-merge | No automatic merging without human approval |
| Bulk approve | No "approve all" button that bypasses review |
| Review metrics | No stats that could gamify review quality |
| Mobile support | Desktop/laptop is the target |

## Traceability

| Requirement | Phase | Plan | Status |
|-------------|-------|------|--------|
| REV-01 | Phase 9 | 09-01 | Complete |
| REV-02 | Phase 9 | 09-03 | Complete |
| REV-03 | Phase 9 | 09-04 | Complete |
| REV-04 | Phase 9 | 09-02, 09-04 | Complete |
| REV-05 | Phase 9 | 09-03 | Complete |
| COM-01 | Phase 10 | 10-02 | Complete |
| COM-02 | Phase 10 | 10-02 | Complete |
| COM-03 | Phase 10 | 10-02 | Complete |
| COM-04 | Phase 10 | 10-01 | Complete |
| COM-05 | Phase 10 | 10-03 | Complete |
| COM-06 | Phase 10 | 10-04 | Complete |
| SUB-01 | Phase 11 | 11-03, 11-04 | Planned |
| SUB-02 | Phase 11 | 11-03, 11-04 | Planned |
| SUB-03 | Phase 11 | 11-03, 11-04 | Planned |
| SUB-04 | Phase 11 | 11-03 | Planned |
| MRG-01 | Phase 12 | 12-01, 12-02, 12-04 | Planned |
| MRG-02 | Phase 12 | 12-03, 12-04 | Planned |
| MRG-03 | Phase 12 | 12-03, 12-04 | Planned |
| SYN-01 | Phase 13 | - | Pending |
| SYN-02 | Phase 13 | - | Pending |
| SYN-03 | Phase 13 | - | Pending |
| STK-01 | Phase 14 | - | Pending |
| STK-02 | Phase 14 | - | Pending |

**Coverage:**
- v1.2 requirements: 23 total
- Mapped to phases: 23/23 (100%)

---

<details>
<summary>v1.1 Requirements (Shipped 2026-02-02)</summary>

### Navigation
- [x] **NAV-01**: When user clicks commit in left column, middle column scrolls to show "you are here" commit at viewport top
- [x] **NAV-02**: Auto-scroll behavior works regardless of current scroll position in either column
- [x] **NAV-03**: Scroll animation is smooth and doesn't jar the user

### UI Polish
- [x] **UI-01**: Top action bar in middle column has reduced opacity (e.g., 0.7) by default
- [x] **UI-02**: Top action bar returns to full opacity on hover
- [x] **UI-03**: Left column has single scrollable area (no nested scroll on "PR Stacks" divider)
- [x] **UI-04**: Files changed section shows line counts as "+123/-45" format
- [x] **UI-05**: Line count statistics accurately reflect additions and deletions

### Design
- [x] **DES-01**: Background colors match Graphite's darker palette from screenshots
- [x] **DES-02**: Addition green is muted/forest green (less saturated than v1.0)
- [x] **DES-03**: Gray hierarchy for file lists and UI elements matches Graphite
- [x] **DES-04**: Diff view backgrounds and text colors match Graphite screenshots
- [x] **DES-05**: Overall color saturation is reduced for professional appearance

### Configuration
- [x] **CFG-01**: User can configure preferred editor path in ISL settings
- [x] **CFG-02**: "Open files" button uses configured editor
- [x] **CFG-03**: Editor configuration persists across sessions
- [x] **CFG-04**: Clear error message if configured editor path is invalid

</details>

---
*Created: 2026-01-23 | Updated: 2026-02-02 for v1.2 milestone*
