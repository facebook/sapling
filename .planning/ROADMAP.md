# Roadmap: Sapling ISL Fork

## Milestones

- ✅ **v1.0 MVP** - Phases 1-5 (shipped 2026-01-22)
- ✅ **v1.1 Refinement & Fixes** - Phases 6-8 (shipped 2026-02-02)
- ✅ **v1.2 PR Review View** - Phases 9-14 (shipped 2026-02-02)

## Overview

v1.2 transforms ISL into a complete PR review experience by adding a dedicated review mode with file-by-file navigation, inline commenting, review submission, and merge capabilities. The architecture extends existing ComparisonView and PRDashboard components rather than building parallel systems, reusing ISL's Jotai state management, GitHub GraphQL integration, and serverAPI messaging patterns.

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

<details>
<summary>✅ v1.1 Refinement & Fixes (Phases 6-8) - SHIPPED 2026-02-02</summary>

**Milestone Goal:** Fix v1.0 issues discovered in usage and refine color scheme to match Graphite more closely.

### Phase 6: Navigation Fixes
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
- [x] 06-02-PLAN.md — Gap closure: move scroll-margin-top to correct CSS element

### Phase 7: UI Polish & Configuration
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
**Plans**: 1 plan

Plans:
- [x] 07-01: TopBar opacity, +X/-Y line counts, editor error handling

### Phase 8: Design Refinement
**Goal**: Color scheme matches Graphite's darker, more muted palette for professional appearance
**Depends on**: Phase 7 (best to see refined behavior with refined colors)
**Requirements**: DES-01, DES-02, DES-03, DES-04, DES-05
**Success Criteria** (what must be TRUE):
  1. Background colors match Graphite's darker palette from reference screenshots
  2. Addition green is muted forest green with reduced saturation
  3. Gray hierarchy for file lists and UI elements matches Graphite
  4. Diff view backgrounds and text colors match Graphite screenshots
  5. Overall color saturation is reduced for professional, easy-on-eyes appearance
**Plans**: 1 plan

Plans:
- [x] 08-01: Graphite color scheme (commit fb5446b649)

</details>

<details>
<summary>✅ v1.2 PR Review View (Phases 9-14) - SHIPPED 2026-02-02</summary>

**Milestone Goal:** Transform ISL into a complete PR review experience with inline commenting, review submission, merge capabilities, and stacked PR navigation. Extends existing ComparisonView and PRDashboard components rather than building parallel systems.

#### Phase 9: Review Mode Foundation + File Tracking
**Goal**: User can enter review mode and navigate files with persistent viewed checkmarks
**Depends on**: Phase 8 (v1.1 complete)
**Requirements**: REV-01, REV-02, REV-03, REV-04, REV-05
**Success Criteria** (what must be TRUE):
  1. User can click "Review" button on PR row to enter focused review mode
  2. Review mode shows file list with navigation in right drawer
  3. User can mark files as "viewed" with checkmarks that persist across sessions
  4. Viewed status automatically resets when PR is updated with new commits
  5. User can navigate file-by-file through diff view using next/previous controls
**Plans**: 4 plans

Plans:
- [x] 09-01-PLAN.md — Review mode state and entry point (Review button on PR rows)
- [x] 09-02-PLAN.md — PR-aware viewed file key (resets on PR update)
- [x] 09-03-PLAN.md — File navigation controls (next/prev buttons)
- [x] 09-04-PLAN.md — PR-aware file review checkmarks

#### Phase 10: Inline Comments + Threading
**Goal**: User can add inline comments on diff lines and interact with existing comment threads
**Depends on**: Phase 9 (review mode foundation)
**Requirements**: COM-01, COM-02, COM-03, COM-04, COM-05, COM-06
**Success Criteria** (what must be TRUE):
  1. User can add inline comments on specific diff lines by clicking line number
  2. User can add file-level comments not tied to specific lines
  3. User can add PR-level general comments to overall conversation
  4. Comments remain pending until review submission (batch workflow prevents premature posting)
  5. User can see and reply to existing comment threads from GitHub
  6. User can resolve/unresolve comment threads with visual collapsed state
**Plans**: 5 plans

Plans:
- [x] 10-01-PLAN.md — Pending comments state foundation (Jotai atoms, localStorage persistence)
- [x] 10-02-PLAN.md — Comment input UI and line click handlers
- [x] 10-03-PLAN.md — Existing comment threads display and reply
- [x] 10-04-PLAN.md — Thread resolution (resolve/unresolve)
- [x] 10-05-PLAN.md — Integration: wire comments into diff view with pending badge

#### Phase 11: Review Submission
**Goal**: User can submit complete review with approval decision and summary text
**Depends on**: Phase 10 (comment infrastructure)
**Requirements**: SUB-01, SUB-02, SUB-03, SUB-04
**Success Criteria** (what must be TRUE):
  1. User can submit review with "Approve" action to approve PR
  2. User can submit review with "Request Changes" action to block merging
  3. User can submit review with "Comment" action (no approval decision)
  4. User can add summary text when submitting review to provide context
  5. All pending comments publish together when review is submitted
**Plans**: 4 plans

Plans:
- [x] 11-01-PLAN.md — Add PR node ID to GraphQL query for mutation API
- [x] 11-02-PLAN.md — Server-side submitPullRequestReview handler
- [x] 11-03-PLAN.md — ReviewSubmissionModal component (summary text + action selection)
- [x] 11-04-PLAN.md — Wire Submit Review button into ComparisonView and review flow

#### Phase 12: Merge + CI Status
**Goal**: User can see CI status and merge PR with strategy selection from review mode
**Depends on**: Phase 11 (review submission, but independent - users can review in ISL and merge in GitHub web UI temporarily)
**Requirements**: MRG-01, MRG-02, MRG-03
**Success Criteria** (what must be TRUE):
  1. User can see CI status (passing/failing/pending) before merging
  2. User can merge PR with strategy selection (merge commit/squash/rebase)
  3. Merge button is disabled when CI failing or required reviews pending
  4. Merge operation shows clear feedback for conflicts or other failures
**Plans**: 4 plans

Plans:
- [x] 12-01-PLAN.md — Extend DiffSummary with mergeability and CI check details
- [x] 12-02-PLAN.md — CIStatusBadge component for detailed CI status display
- [x] 12-03-PLAN.md — MergePROperation and merge state logic
- [x] 12-04-PLAN.md — MergeControls UI integrated into review mode

#### Phase 13: Sync/Rebase
**Goal**: User can keep PR in sync with latest main and rebase stack without leaving review mode
**Depends on**: Phases 9-11 (must preserve drafts and invalidate line-based state correctly)
**Requirements**: SYN-01, SYN-02, SYN-03
**Success Criteria** (what must be TRUE):
  1. User can sync current branch with latest main via button in review mode
  2. User can rebase all open PRs in stack on latest main
  3. User sees clear feedback during sync/rebase operation (progress, conflicts, completion)
  4. System warns user before sync if pending comments exist that may become invalid
  5. Viewed file status and draft comments handle rebases gracefully
**Plans**: 5 plans

Plans:
- [x] 13-01-PLAN.md — SyncPROperation class for gh pr update-branch
- [x] 13-02-PLAN.md — Sync warning detection (pending comments, viewed files)
- [x] 13-03-PLAN.md — SyncPRButton with warning modal in review mode
- [x] 13-04-PLAN.md — Stack rebase via existing RebaseAllDraftCommitsOperation
- [x] 13-05-PLAN.md — Sync progress feedback in review mode

#### Phase 14: Stacked PR Navigation
**Goal**: User can navigate between PRs in a stack without exiting review mode
**Depends on**: Phase 9 (review mode foundation, independent of comments/merge)
**Requirements**: STK-01, STK-02
**Success Criteria** (what must be TRUE):
  1. User can see stacked PR relationships visualized in review mode (A -> B -> C)
  2. User can navigate between PRs in a stack without exiting review mode
  3. Stack visualization highlights current PR and shows sync status
  4. Switching between stack PRs preserves review progress (viewed files, pending comments)
**Plans**: 3 plans

Plans:
- [x] 14-01-PLAN.md — Stack context atom (currentPRStackContextAtom in PRStacksAtom.ts)
- [x] 14-02-PLAN.md — StackNavigationBar component with PR pill buttons
- [x] 14-03-PLAN.md — State preservation verification and human testing

</details>

## Progress

**Execution Order:** Phases execute in numeric order: 9 -> 10 -> 11 -> 12 -> 13 -> 14

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Layout Foundation | v1.0 | 3/3 | Complete | 2026-01-22 |
| 2. Stack Column | v1.0 | 3/3 | Complete | 2026-01-22 |
| 3. Commit Tree | v1.0 | 2/2 | Complete | 2026-01-22 |
| 4. Details Panel | v1.0 | 2/2 | Complete | 2026-01-22 |
| 5. Diff Polish | v1.0 | 1/1 | Complete | 2026-01-22 |
| 6. Navigation Fixes | v1.1 | 2/2 | Complete | 2026-01-23 |
| 7. UI Polish & Configuration | v1.1 | 1/1 | Complete | 2026-02-02 |
| 8. Design Refinement | v1.1 | 1/1 | Complete | 2026-01-27 |
| 9. Review Mode Foundation + File Tracking | v1.2 | 4/4 | Complete | 2026-02-02 |
| 10. Inline Comments + Threading | v1.2 | 5/5 | Complete | 2026-02-02 |
| 11. Review Submission | v1.2 | 4/4 | Complete | 2026-02-02 |
| 12. Merge + CI Status | v1.2 | 4/4 | Complete | 2026-02-02 |
| 13. Sync/Rebase | v1.2 | 5/5 | Complete | 2026-02-02 |
| 14. Stacked PR Navigation | v1.2 | 3/3 | Complete | 2026-02-02 |
