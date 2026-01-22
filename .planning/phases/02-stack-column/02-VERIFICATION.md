---
phase: 02-stack-column
verified: 2026-01-22T09:53:06Z
status: passed
score: 9/9 must-haves verified
re_verification: false
---

# Phase 2: Stack Column Verification Report

**Phase Goal:** Users can navigate the PR stack by clicking commits/PRs to checkout
**Verified:** 2026-01-22T09:53:06Z
**Status:** passed
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Clicking any commit or PR in the stack checks it out (not just the pull button) | ✓ VERIFIED | PRRow component (line 376) has onClick={handleCheckout}, StackCard header (line 249) has onClick={handleStackCheckout} |
| 2 | "main" branch appears at top of stack with pull/checkout button | ✓ VERIFIED | MainBranchSection component (lines 36-97) renders above stack content (line 139), has "Go to main" button (lines 82-94) |
| 3 | origin/main is visually distinct from other items in the stack | ✓ VERIFIED | Commit.tsx line 491 applies 'origin-main-commit' class, lines 525-529 render origin-main-badge, CommitTreeList.css lines 291-327 provide visual styling |

**Score:** 3/3 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `addons/isl/src/PRDashboard.tsx` | Click-to-checkout on rows/cards + MainBranchSection | ✓ VERIFIED | 436 lines, exports PRDashboard, has MainBranchSection (36-97), PRRow with handleCheckout (354-359), StackCard with handleStackCheckout (198-207) |
| `addons/isl/src/PRDashboard.css` | Hover states, sticky positioning | ✓ VERIFIED | 307 lines, contains cursor:pointer (lines 108, 164), sticky positioning (line 31), hover states (lines 111-113, 167-169) |
| `addons/isl/src/CommitTreeList.tsx` | isOriginMain detection | ✓ VERIFIED | Contains isOriginMain function (lines 47-55), passes isMainBranch to Commit component (line 205) |
| `addons/isl/src/CommitTreeList.css` | origin-main visual styling | ✓ VERIFIED | 328 lines, contains origin-main-commit class (lines 291-304), origin-main-badge styling (lines 306-322) |
| `addons/isl/src/Commit.tsx` | isOriginMain prop, badge rendering | ✓ VERIFIED | Accepts isOriginMain prop (line 159), applies class (line 491), renders badge (lines 525-529) |

### Key Link Verification

| From | To | Via | Status | Details |
|------|-----|-----|--------|---------|
| PRRow | GotoOperation | useRunOperation + onClick handler | ✓ WIRED | Import on line 29, runOperation(new GotoOperation(succeedableRevset(headHash))) on line 358, onClick={handleCheckout} on line 376 |
| StackCard | GotoOperation | useRunOperation + onClick handler | ✓ WIRED | Import on line 29, runOperation(new GotoOperation(...)) on line 206, onClick={handleStackCheckout} on line 249 |
| MainBranchSection | PullOperation + GotoOperation | useRunOperation chain | ✓ WIRED | Imports on lines 28-29, await runOperation(new PullOperation()) then runOperation(new GotoOperation(...)) on lines 64-65 |
| Commit | isOriginMain prop | CommitTreeList detection | ✓ WIRED | isOriginMain function in CommitTreeList.tsx (47-55), passed to Commit (line 205), used in className (line 491) and badge render (line 525) |
| PRDashboard | App.tsx | Component import and render | ✓ WIRED | PRDashboard exported (line 99), imported in App.tsx (line 25), rendered in left panel (line 114) |
| CommitTreeList | App.tsx | Component render | ✓ WIRED | CommitTreeList exported, imported in App.tsx, rendered in middle panel (line 136) |

### Requirements Coverage

| Requirement | Status | Blocking Issue |
|-------------|--------|----------------|
| STACK-01: Clicking a commit/PR checks it out | ✓ SATISFIED | None - PRRow and StackCard both have click handlers wired to GotoOperation |
| STACK-02: "main" appears at top with pull/checkout button | ✓ SATISFIED | None - MainBranchSection implemented with sticky positioning and go-to-main button |
| STACK-03: origin/main is visually highlighted | ✓ SATISFIED | None - isOriginMain detection + CSS styling + badge component all present |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| PRDashboard.tsx | 337 | placeholder="Enter label..." | ℹ️ Info | Benign - actual placeholder text for input field |
| CommitTreeList.tsx | 252 | TODO comment about recoil subscription | ℹ️ Info | Not blocking - unrelated implementation note |

**No blocking anti-patterns found.**

### Human Verification Required

#### 1. Visual Appearance Check

**Test:** Open ISL in browser, view PR stack with multiple items
**Expected:** 
- PR rows and stack headers show pointer cursor on hover
- Main branch section stays at top when scrolling
- origin/main commits have blue left border, subtle background gradient, and "main" badge
- Current PR/commit has blue accent border
- Loading state shows spinner during checkout

**Why human:** Visual polish and responsive behavior can't be verified programmatically

#### 2. Click-to-Checkout Flow

**Test:** Click on a PR row, then click on a stack header, then click "Go to main"
**Expected:**
- Clicking PR row checks out that PR's head commit
- Clicking stack header checks out the top PR
- Go to main pulls latest and checks out main
- Already checked-out items don't trigger redundant operations
- Child elements (PR number link, view changes button) don't trigger checkout

**Why human:** Need to verify actual checkout operations occur and Git state changes

#### 3. Sync Status Accuracy

**Test:** Compare main branch section sync status with actual git status
**Expected:**
- Shows "Updates available" when remote/main differs from local main
- Shows "You are here" when currently on main
- Shows "Up to date" when local main matches remote

**Why human:** Requires comparing UI state with actual repository state

### Gaps Summary

**No gaps found.** All must-haves verified:

✓ **Click-to-checkout on PR rows:** PRRow component has onClick handler (line 376) that calls handleCheckout (lines 354-359), which runs GotoOperation with the PR's head hash. Smart child filtering prevents clicks on PR number link and view changes button from triggering checkout.

✓ **Click-to-checkout on stack headers:** StackCard component has onClick handler (line 249) on the header that calls handleStackCheckout (lines 198-207), checking out the top PR's head commit. Smart filtering prevents expand button and action buttons from triggering checkout.

✓ **Main branch at top:** MainBranchSection component (lines 36-97) renders between header and content (line 139), uses sticky positioning (PRDashboard.css line 31), shows branch name, sync status, and "Go to main" button that pulls then checks out (lines 64-65).

✓ **Visual current indicators:** CSS classes pr-row-current (lines 171-179) and stack-card-current (lines 133-139) applied conditionally, showing blue accent border and subtle background.

✓ **Loading states:** inlineProgress tracked via useAtomValue(inlineProgressByHash()), shows loading icon and disables interaction during operations.

✓ **origin/main highlighting:** isOriginMain function (CommitTreeList.tsx lines 47-55) detects main branch commits, isOriginMain prop passed to Commit component (line 205), applies origin-main-commit class (Commit.tsx line 491), renders badge with icon and "main" text (lines 525-529), CSS provides blue left border and gradient background (CommitTreeList.css lines 291-327).

✓ **Wiring complete:** All imports present, operations connected, components rendered in App.tsx, TypeScript compiles successfully (verified with yarn build).

✓ **No stubs:** All implementations substantive - PRRow 103 lines, StackCard 141 lines, MainBranchSection 61 lines, isOriginMain 8 lines, all with real logic and proper error handling.

✓ **Visual consistency:** Uses Graphite color variables throughout (--graphite-accent, --graphite-bg, --graphite-border), consistent with Phase 1 design system.

---

_Verified: 2026-01-22T09:53:06Z_
_Verifier: Claude (gsd-verifier)_
