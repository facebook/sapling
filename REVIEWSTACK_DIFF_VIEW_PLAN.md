# ReviewStack Diff View Plan

## Goal

Add two user preferences to the ReviewStack pull request view:

- Select a split or unified diff layout.
- Hide changes that contain only whitespace.

Keep the current comment, syntax, and file-navigation behavior.

## Decision

Extend the current ReviewStack renderer. Do not replace it with a third-party renderer.

The current renderer already supports these features:

- Inline and multiline comments
- Comments from old versions
- Resolved comments
- Syntax highlighting
- Expandable context
- Large-file placeholders
- GitHub line-position mapping

A library migration adds risk to each feature. The requested controls do not require a complete renderer replacement.

`react-diff-view` is the best third-party fallback. It supports split and unified layouts and provides widgets for comments.

Other options include `react-diff-viewer`, `diff2html`, and the Monaco Diff Editor. These options need more comment integration work.

## Delivery Strategy

Use two small pull requests. Deploy and evaluate each pull request separately.

### Phase 1: Whitespace Preference

Add a **Hide whitespace changes** control above the diff.

Pass the preference to the existing diff worker. Use the whitespace option in the current `jsdiff` pipeline.

Do not filter rendered rows after diff generation. Row filtering can produce incorrect line numbers and comment positions.

Store the preference in browser storage. Apply the preference to all files in the pull request.

Keep commenting available only when GitHub line positions remain valid. Show a clear message if a filtered diff cannot support comments safely.

### Phase 2: Unified Layout

Add a **Split / Unified** layout selector above the diff.

Keep the current split renderer unchanged. Add unified rendering as a second presentation path for the same diff model.

In unified mode, show these columns:

- Old line number
- New line number
- Diff content

Show removed lines before added lines. Keep comments attached to their GitHub diff side and line number.

Store the layout preference in browser storage. Switching layouts must not reload the pull request.

## Required Behavior

Both layouts must support:

- Added files
- Removed files
- Modified files
- Renamed files
- Binary-file placeholders
- Expanded context
- Syntax highlighting
- Inline comments on the left and right sides
- Multiline comment selection
- Pending comments
- Resolved comments
- Comments from old versions
- Large-file placeholders

The whitespace preference must support:

- Changes to indentation only
- Changes to trailing whitespace only
- Blank-line changes
- Changes that contain both whitespace and code edits
- Files that become unchanged after whitespace filtering

## Test Plan

Add focused unit tests for the worker options and unified row mapping.

Add browser tests for these workflows:

1. Switch between split and unified layouts.
2. Reload the page and make sure that the selected layout remains active.
3. Hide and show whitespace-only changes.
4. Add a comment on an added line in each layout.
5. Add a comment on a removed line in each layout.
6. Select a multiline comment range in each layout.
7. Show historical and resolved comments in each layout.
8. Expand hidden context in each layout.
9. Open a large file and load its diff in each layout.

Use realistic patches in the tests. Do not replace the diff or comment logic with mocks.

## Rollout and Rollback

Deploy the whitespace preference first. Evaluate it before work starts on unified layout.

Keep split layout as the default during the first unified-layout release.

The existing split renderer stays available as a fallback. A rollback must require only an image change.

Monitor browser errors and GitHub comment-mutation errors after each release.

## Main Risks

The unified layout can attach a comment to the wrong diff side. Tests must cover comments on added and removed lines.

Whitespace filtering can change diff hunks. The implementation must preserve GitHub-compatible line positions before it enables commenting.

Layout changes can affect file-header stacking and comment popovers. Browser tests must cover scrolling and open menus.

Large diffs can use too much memory. The implementation must retain the current worker and placeholder behavior.

## Completion Criteria

The work is complete when both preferences are available and remain active after a reload.

All existing comment actions must work in both layouts. The production browser suite must pass before each deployment.
