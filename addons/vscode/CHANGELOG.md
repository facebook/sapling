# Changelog

## 0.1.34

### Dec 14 2023

- Multiple improvements to Interactive Split
  - Handles binary and copied/renamed files
  - Shows file flag changes, like making a file executable
  - Show file status (added/removed) in file header
  - Long filenames wrap to multiple lines and don't break up the left/right arrows
  - Show how a file was renamed or copied
- Don't run status refreshing commands while an operation is running, to fix lots of random files being shown.
- Fix conflicts sometimes not being shown when they should be

## 0.1.33

### Dec 11 2023

- Add UI zoom setting
- Confirm when uncommitting
- Add date to temporary commit titles
- Increase number of uncommitted files shown at once
- Previewing system was rewritten with several improvements, such as commits not appearing duplicated during a rebase

## 0.1.32

### Nov 30 2023

- Added "Combine" button when selecting multiple adjacent commits in a stack, to fold them together.
  - The combine is previewed before running, so you can adjust the combined commit message.
- Added Bulk actions dropdown to the top bar to add actions that act on all commits at once
  - "rebase all draft commits" button to bulk rebase onto suggested locations
  - "clean up all" button to hide all closed commits
  - "select all" to select all draft commits, so you can take further actions in the sidebar
  - Added shortcut to select all commits
- When multiple commits are selected, allow rebasing them all from the selection sidebar
- Commit titles are no longer directly focusable, so the UI doesn't show conflicting highlights. Buttons have better Aria labels to compensate.
- Use more consistent focus colors
- Fix "Temporary Commit" appearing in commit title by default
- [#781](https://github.com/facebook/sapling/pull/781): Increase width of split column on large screens
- [#782](https://github.com/facebook/sapling/pull/782): Reduces number of PRs fetched from GitHub to improve performance

## 0.1.31

### Nov 16 2023

- Added more keyboard shortcuts and a list of shortcuts openable via Shift-?
- Add config for amend restacking. This now defaults to "Always" instead of "No Conflict"
- Allow drag & drop rebase when uncommitted changes have been optimistically removed
- Fix empty titles eating into the summary
- Add explicit ">" button next to commits to open them in the sidebar
- Remove duplicates from values in typeaheads
- Simplify "Hide" context menu item for non-stack commits
- Rewrote edited messages implementation, fixing some weird behaviors

## 0.1.30

### Nov 8 2023

- Bulk query generated files in batches of 400, so files are sorted by status before pagination in groups of 25.
  - Also warns if there are too many files to correctly sort in one batch.
- Improve handling of VS Code "modern" themes
- Update icon for warning signals to be more consistent
- Add border to icon-style buttons
- Press backspace to preview hiding a commit
- Make top bar sticky as you scroll
- Add current stack base as a suggested rebase target
- Show all changed files in the commit's files list in the sidebar
- Allow opening a diff view of a deleted file
- Allow reverting added files in the head commit

## 0.1.29

### Oct 26 2023

- Add initial support for special handling for generated files
  - Currently checks for "&#0064;generated" in the head of any files
  - Eventually this will also be configurable by path to look for commons files
  - Generated files are sorted below regular files in the list of uncommitted changes,
    and the section is collapsed by default
  - Generated files' content is hidden by default in the comparison view
  - Also supports _partially generated_ files by looking for "&#0064;partially-generated". These files are marked as generated, but not collapsed.
- Improve how obsolete commits behave with operations. If a commit is already obsolete (has a newer version from some operation, such as amending), operations will act on it specifically, instead of using the latest successor. This makes drag-and-drop rebase more predicatable when dragging onto commits which are obsolete and fixes some weird behaviors.
- Add options to goto, rebase to same public base, and rebase on top when downloading (pulling) commits
- Remember UI layout (commit info sidebar expansion/width) when reopening the page
- Smarter auto-collapsing of the commit info sidebar, such as when resizing the window
- Use a teal color for missing files to differentiate them from untracked files
- Auto-close the cwd selector when changing the cwd
- Fix weird padding on filenames in the comparison view
- Fix filenames like #backup# rendering incorrectly
- Merge commit messages when using Edit Stack
- Use your commit template when making commits with quick commit or split
- Fix minor rendering issues

## 0.1.28

### Oct 12 2023

- Correct files appear in commit info while `commit` is running
- Added a slight background color to icon buttons, to distinguish them from just text
- Add a button to open the Split UI next to the current commit
- Make the uncommit button less noticeable
- Fix file names overflowing in the comparison view and split view
- Add buttons to expand/collapse all files in the comparison view
- Add a "compact" mode config option. This makes commits not wrap onto multiple lines as early, which increases the density of commits visible at the same time.
- Remove language setting, since it we don't yet have any other translations.
- Improvements to file chunk selection UX
- Fix discarding subsets of files not actually deleting them from disk

## 0.1.27

### Sep 22 2023

- Confirm unsaved message changes before opening the split UI
- Use the latest remote message for commits in the split UI
- Hold alt key to quickly show full file paths
- Fix comparison view not having any margins
- Add left/right arrows to navigate pages in uncommitted changes list when there are more than 25 files
- Fix commit info sidebar shrinking by 2px in some cases

## 0.1.26

### Sep 14 2023

- Disallow opening split from context menu when you have uncommitted changes, since they may be lost
- Fix left arrow overflowing on hover

## 0.1.25

### Sep 13 2023

- Many UI and UX changes to the _Interactive Split_ feature. While still labeled _Beta_, it's much more useable now.
- Added Syntax highlighting for the Split view.
- Split can now be accessed on any single commit by right click
- When using Split from the _Edit Stack_ menu, you can more easily select a range of commits to split
- Temporarily removed the _Files_ tab from edit stack, until it can be polished similar to the _Split_ tab.
- New icon in the "go to" button

## 0.1.24

### Sep 07 2023

- Suggested rebase button to make rebases across many commits easier
- Button to "shelve" commits, and a menu to list and unshelve them again
- Make the list of uncommitted changes scrollable and truncate if very long
- Show a banner if the list of files in a commit has been truncated
- Fixed submit stack spinner spinning if you submit any visible stack
- Fix "you are here" and uncommitted changes sometimes not being visible
- Inline Blame now shows the author name and appears by default
- Now in beta: _Interactive Split_ UI as part of the _Edit Stack_ menu.
  This lets you make multiple commits out of a single large commit.
  We expect this feature to change in the next few releases.
- Now in beta: _Partial "Chunk" Selection_ of uncommitted changes.
  Click the Chunk selection button next to files to open a selection view of the file's hunks.
  Only selected hunks will be included in the next commit or amend.

## 0.1.23

### Aug 28 2023

- Fix commits info view sometimes becoming read only and buttons not working
- Add border to checkboxes/radios in all themes for improved viewability
- Improved error messages for some types of errors
- Made font sizes and button sizes more consistent

## 0.1.22

### Aug 23 2023

- Syntax highlighting in comparison view
- Add option to "amend changes to here" when right clicking a commit
- Add ellipsis to file path overflow
- Collapse some files by default in the comparison view for performance
- Timeout some commands to prevent hanging issue
- Fix commit highlighting on hover not going away
- Improve drag target size for commits in drag and drop rebase
- Improve rendering comparison view file banners, such as "this file was renamed"

## 0.1.21

### Aug 15 2023

- Selection and copying from comparison view stays within the before/after sides
- Support collapsing files in the comparison view
- More consistent styling in the comparison view
- Add succession tracking for smoother previews of goto
- Propagate unsaved edited commit messages when a commit is amended
- Improved optimistic state when hiding commits
- Add "cleanup" button to quickly hide landed commits
- Fix commit download box missing some inputs
- Fix tab ordering in commit info view

## 0.1.20

### Aug 07 2023

- Add badge for review decision for GitHub PRs
- Add button to open all changed files in a commit
- Add debugging tools
- Fix missing help buttons if commits fail to load

## 0.1.19

### Jul 25 2023

- Add context menu to files to copy paths and open diff views
- Fix visual overflow in commit messages
- Allow amending with only message changes

## 0.1.18

### Jul 20 2023

- Allow pressing Enter to quick commit
- Add button to open a file in the comparison view
- Add tooltip to copy filenames in the comparison view
- Fix white line artifact when selecting a commit
- Fix PR links in blame hover

## 0.1.17

### Jul 18 2023

- Add dropdown to pull a specific commit from remote
- Experimental partial commit UI hidden behind `isl.experimental-features` config
- Thanks to [@alex-statsig](https://github.com/alex-statsig) for several contributions in this release:
  - [Experimental inline blame annotations, disabled in settings by default](https://github.com/facebook/sapling/pull/640)
  - [Fix diff views being backwards](https://github.com/facebook/sapling/pull/637)
  - [Fix missing data until first poll](https://github.com/facebook/sapling/pull/638)
  - [Fix github CI status check](https://github.com/facebook/sapling/pull/651)

## 0.1.16

### May 31 2023

- Add "Edit stack" to reorder, drop, or fold stacked commits
- CI signal badge is now responsive and more obvious
- Display short hashes in the commit line arguments
- Public commits are indicated in the commit view
- Disallow editing fields or amending changes for public commits
- Sidebar revert button now reverts to the parent commit
- Show a spinner during code review submitting
- Respect theme colors like "Solarized"
- Update VSCode UI toolkits to use rounded button
- Fix hide operation to not hide successors
- Fix tooltip alignment in some cases
- Fix `isl-server` crash when `xdg-open` is not installed

## 0.1.15

### May 09 2023

- Add repo selector if multiple workspace folders are mounted
- Add "View Changes" context menu action on commits to quickly diff their changes
- Show diff badges inline on large displays to better use horizontal space
- Experimental stack editing UX hidden behind `isl.experimental-features` `sl` config

## 0.1.14

### May 03 2023

- Fix tooltips not disappearing (such as on pull button)
- Use normal font-smoothing for more readable text
- Hide uncommit button on closed PRs

## 0.1.13

### Apr 26 2023

- Customize how changed file paths are displayed: minimal, full file path, tree view, or fish-shell-style
- Copy quick commit form title into full commit form when clicking "Commit as..."
- Fix tooltips wrapping text mid-word
- Allow repos cloned without http prefix

## 0.1.12

### Apr 06 2023

- Reduce visual padding in the tree to improve information density
- Show copied/renamed files
- Add revert button to files on non-head commits
- Use more consistent custom icon for pending CI tests
- Reduce number of spinners while running goto
- Fix line numbers wrapping in the comparison view
- Fix text overflow in tooltips
- Fix truncation for long file names
- Fix vscode webview getting stuck with "Webview is disposed" error when reopened

## 0.1.11

### Mar 24 2023

- Allow submitting PRs as drafts and showing whether a PRs is a draft
- Option to put ISL in the vscode sidebar instead of in the editor area
- Allow selecting multiple commits with cmd/shift click
- Use arrow keys to change selected commit
- Don't show diff button next to merge conflicts
- Improve behavior when there are no commits in the repo
- Click on line numbers in the comparison view to open the file
- Fix optimistic state sometimes getting stuck when queueing commands
- Fix tooltips persisting and getting in the way
- Fix ISL not loading when all commits in the repo are older than 2 weeks

## 0.1.10

### Feb 23 2023

- Added revert button to VS Code SCM Sidebar files
- Added button to open diff view for VS Code SCM Sidebar files
- Use --addremove flag when committing/amending so untracked files are included
- Fix ssh:// upstream paths for GitHub repos not being detected as valid repos
- Better styling of Load More button and commit graph

## 0.1.9

### Feb 09 2023

- Fix sending messages to disposed webviews which caused ISL to stop working
- Add context menu support
- Forget button for added files & delete button for untracked files
- Button load older commits, only show recent commits at first
- Show copied/renamed files in the comparison view
- Double click a commit to open the commit info sidebar
- `sl hide` commits via context menu action
- Support aborting operations
- Use minimal path name for changed files
- Show commit time next to public commits
- Disable pull button while pull is running
- Add color and icon next to filenames in comparison view
- Fixes for color and wrapping in the changed files list

## 0.1.8

### Feb 09 2023

- ISL no longer crashes when a language other than English is selected in VS Code: <https://github.com/facebook/sapling/issues/362>.
- Added an ISL menu button to the source control panel: <https://github.com/facebook/sapling/commit/538c6fba11ddfdae9de93bf77cffa688b13458c0>.
- Updated the Sapling icon: <https://github.com/facebook/sapling/commit/2f7873e32208d4cd153b7c1c1e27afe19e815cf0>.

## 0.1.7

### Dec 12 2022

- Fixed an issue where we were stripping the trailing newline in the output to `sl cat`, which caused the VS Code extension to constantly report that the user had modified a file by adding a newline to the end: <https://github.com/facebook/sapling/commit/f65f499ba95a742444b61cb181adb39d2a3af4c2>.

## 0.1.6

### Dec 09 2022

- Fixed an issue with path normalization that was preventing extension commands from working on Windows because files were not recognized as part of a Sapling repository: <https://github.com/facebook/sapling/commit/206c7fbf6bc94e7e5940630b812fba7dcd55140e>.
- Cleaned up the instructions on how to use the extension in the README: <https://github.com/facebook/sapling/commit/4ee418ca7aab519b1b4f96edd0991311e8c6b03f>
- Fixed an issue where the **See installation docs** button in ISL failed to open the installation docs: <https://github.com/facebook/sapling/issues/282>.

## 0.1.5

### Nov 30 2022

- Did not realize a release and pre-release cannot share a version number. Re-publishing the 0.1.4 pre-release with 4c29208c91256f4306aec9f0e9ec626e96ea3cba included as an official release.

## 0.1.4

### Nov 29 2022

- Fixed #282: Add config option to set what `sl` command to use
- More reliably detect command not found on Windows

## 0.1.3

### Nov 21 2022

- Support GitHub enterprise and non-GitHub repos in ISL
- Add revert button next to uncommitted changes in ISL
- Add repo/cwd indicator at the top of ISL
- Show a spinner while the comparison view is loading
- Fix tooltips being misaligned in corners
- Make styling more consistent between web and VS Code

## 0.1.2

### Nov 16 2022

- Fix the comparison view not scrolling
- Show an error in ISL if Sapling is not yet installed

## 0.1.1 - Initial release

### Nov 14 2022

###

Features:

- Interactive Smartlog (ISL) embedded as a webview
- Simple support for VS Code SCM API, including showing changed files
- Diff gutters in changed files
- VS Code Commands to open diff views for the current file
