---
phase: 14
plan: 02
subsystem: review-mode-ui
tags: [typescript, react, css, navigation, stack-visualization]
requires:
  - phase: 14
    plan: 01
    feature: "currentPRStackContextAtom for stack context"
provides:
  - "StackNavigationBar component for PR stack navigation"
  - "Visual stack representation with clickable pills"
  - "Current PR highlighting and position indicator"
affects:
  - phase: 14
    plan: 03
    reason: "Stack navigation bar ready for dropdown enhancement"
tech-stack:
  added: []
  patterns:
    - "Conditional rendering based on stack context"
    - "Tooltip-wrapped pill buttons for navigation"
    - "Optimistic navigation with enterReviewMode"
key-files:
  created: []
  modified:
    - path: "addons/isl/src/ComparisonView/ComparisonView.tsx"
      changes: "Added StackNavigationBar component and integrated into ComparisonView"
    - path: "addons/isl/src/ComparisonView/ComparisonView.css"
      changes: "Added stack navigation bar styling"
decisions:
  - id: "14-02-A"
    choice: "Place stack navigation bar between header and merge controls"
    rationale: "Logical position showing context before actions, matches information hierarchy"
  - id: "14-02-B"
    choice: "Use pill-shaped buttons with tooltips for PR navigation"
    rationale: "Compact visual representation, tooltips show full titles on hover"
  - id: "14-02-C"
    choice: "Skip navigation if headHash is empty (PR not in summaries)"
    rationale: "Graceful degradation for missing PR data, prevents errors"
metrics:
  duration: "90s"
  completed: "2026-02-02"
---

# Phase 14 Plan 02: Stack Navigation Bar UI Summary

**One-liner:** Clickable stack navigation bar with pill buttons showing PR numbers, current position, and merge status

## What Was Built

Created the StackNavigationBar component that displays all PRs in a stack with visual navigation:

1. **StackNavigationBar Component** (`ComparisonView.tsx`)
   - Reads `currentPRStackContextAtom` for stack context
   - Only renders for multi-PR stacks in review mode
   - Returns `null` for single PRs or when not in review mode
   - Maps stack entries to pill buttons with tooltips

2. **Navigation Interaction**
   - Clicking a non-current PR calls `enterReviewMode(prNumber, headHash)`
   - Current PR is highlighted and disabled (can't click)
   - Empty headHash PRs are disabled (graceful degradation)
   - Tooltips show full PR titles on hover (500ms delay)

3. **Visual Feedback**
   - Current PR: Primary color background and foreground
   - Merged PRs: Check icon + 0.6 opacity
   - Hover effect: Border highlights with primary color
   - Position indicator: "1 / 3" format on right side

4. **CSS Styling** (`ComparisonView.css`)
   - Subtle background bar (`--graphite-bg-subtle`)
   - Pill-shaped buttons (12px border-radius)
   - Proper spacing and transitions (0.15s ease)
   - Tabular numbers for position indicator
   - Uppercase "STACK" label with letter-spacing

## Technical Decisions

### Decision 14-02-A: Stack Bar Placement

**Context:** Where should the stack navigation bar appear in the UI?

**Options:**
1. Above header (top of view)
2. Between header and merge controls
3. Below merge controls

**Choice:** Between header and merge controls

**Rationale:**
- Provides context immediately after viewing general comparison options
- Appears before action buttons (merge/sync), matching information → action flow
- Doesn't interfere with primary header controls
- Naturally collapses (returns null) when not needed

### Decision 14-02-B: Pill Button Navigation

**Context:** How to represent PRs in the stack visually?

**Options:**
1. Dropdown select menu
2. Pill-shaped buttons with tooltips
3. Full PR cards in sidebar

**Choice:** Pill-shaped buttons with tooltips

**Rationale:**
- Compact: Shows all PRs at a glance without extra clicks
- Visual: Current PR immediately obvious with highlight color
- Contextual: Tooltips provide full titles without cluttering
- Familiar: Matches GitHub's label/tag UI patterns
- Efficient: Single click to navigate to any PR

### Decision 14-02-C: Empty HeadHash Handling

**Context:** What if a PR in the stack isn't in allDiffSummaries (missing headHash)?

**Options:**
1. Hide the PR from navigation bar
2. Show but disable the button
3. Show with placeholder data and allow click

**Choice:** Show but disable the button (headHash check in onClick)

**Rationale:**
- Maintains stack completeness (shows all PRs)
- Prevents errors from calling enterReviewMode with empty hash
- User sees the stack structure even if some PRs aren't loaded
- Graceful degradation without hiding information

## Integration Points

**Inputs:**
- `currentPRStackContextAtom`: Stack context with entries, position, state
- `reviewModeAtom`: Active state check (implicit via stackContext)

**Outputs:**
- `enterReviewMode(prNumber, headHash)`: Navigation to different PR
- Rendered UI: Stack visualization bar

**Component Tree:**
```
ComparisonView
├── ComparisonViewHeader
├── StackNavigationBar ← NEW
│   ├── Tooltip (per PR)
│   │   └── Button (pill)
│   │       └── Icon (check for merged)
│   └── Position indicator
├── MergeControls (if review mode)
└── Review mode toolbar
```

## Verification Results

✅ **Build:** `yarn --cwd addons/isl build` succeeded
✅ **Component Defined:** StackNavigationBar function exists
✅ **Component Used:** Rendered in ComparisonView JSX
✅ **CSS Styles:** `.stack-navigation-bar` and related classes added
✅ **Imports:** currentPRStackContextAtom and enterReviewMode imported
✅ **Pattern Matching:** `useAtomValue.*currentPRStackContextAtom` present
✅ **Pattern Matching:** `enterReviewMode\(` call present

## User Experience

**Scenario: Developer reviewing stacked PRs**

1. Open PR #123 in review mode (part of 3-PR stack)
2. See stack navigation bar below header:
   ```
   STACK  [#121] [#122] [#123*] [#124]     3 / 4
   ```
   - #123 highlighted (current)
   - Hover #122 to see title: "Add user authentication"
3. Click #122 → Navigates to PR #122 in review mode
4. Stack bar updates: #122 now highlighted
5. See merged PR #121 with check icon and dimmed

**Visual States:**
- Default pill: Light background, subtle border
- Hover: Border highlights blue
- Current: Blue background, white text
- Merged: Check icon, 60% opacity
- Disabled: No hover effect, can't click

## Deviations from Plan

None - plan executed exactly as written.

## Next Phase Readiness

**Phase 14-03 (Stack Label + Dropdown):**
- ✅ Stack navigation bar renders in correct position
- ✅ Pill buttons can be replaced/enhanced with dropdown
- ✅ Stack context available via currentPRStackContextAtom
- ✅ CSS structure supports additional dropdown styling

**Blockers:** None

**Notes:**
- Current implementation focuses on horizontal pill display
- Phase 14-03 will add dropdown for overflow scenarios
- Stack label "STACK" can be enhanced with custom labels later
