# Changelog

## 0.1.70

- Reduced fetching frequency for better performance
- Go to time in download commit menu

## 0.1.69

- More efficient queries for information from Github

## 0.1.68

- Add a setting to completely hide cwd-irrelevant commits
- Add button to swap two commits in the Split UI

## 0.1.67

- Add `--debug` and `--verbose` optional args for help debugging
- Show command output for stack editing

## 0.1.66

### Jan 30 2025

- Made Absorb feature enabled by default (not experimental)
- New features and polish for Absorb
  - syntax highlighting
  - send changes to a specific commit
  - visual changes
- Fix loaded context lines in comparisons sometimes being misaligned

## 0.1.65

### Jan 16 2025

- Add "Keep Rebased Commits on Abort" checkbox
- Experimental "absorb" feature to distribute your changes into your stack, disabled by default
- Improve error messages from spawned commands
- Fix changes with >1000 files not rendering files after the first 1000
- Don't load public commit changed files by default for performance
- Load older commits with Commit Cloud, in supported repos

## 0.1.64

### Nov 19 2024

- Sapling commands now show progress bars (requires a recent Sapling version)

## 0.1.63

### Nov 11 2024

- Improvements to warning diagnostics when submitting for review

## 0.1.62

### Nov 5 2024

- Fixed ISL not respecting your VS Code theme

## 0.1.61

### Oct 31 2024

- Made Changed Files more visually compact

## 0.1.60

### Oct 14 2024

- Optimized how files in commits are fetched, reducing timeouts and improving startup time. Complex commit history fetch times are reduced by 90% or more.
- Improved generated file detection in some cases by looking further in the file contents.
- Warn when queued commands are cancelled due to a failure of a running command, and show which commands exactly were cancelled.
- Recover commit messages from cancelled queued commands, and put them back as unsaved edits so they can be retried.
- Fix some UI elements missing a background

## 0.1.59

### Sep 26 2024

- Automatically check VS Code diagnostics when submitting for review, and warn if there are error signals
- Performance improvement to fetching commits from sl, reducing timeout errors

## 0.1.58

### Sep 16 2024

- Fix focus preservation sometimes causing keystrokes in VS Code cmd-p menu to click buttons in ISL
- Reduce excessive padding on the split modal

## 0.1.57

### Sep 9 2024

- Unsaved edited commit messages are persisted across restarts
- Deemphasize commits that only change files outside your cwd. Useful for large repos where you want to focus on changes in a specific subfolder.
- Show the Uncommit button on commits in the middle of the stack, and warn about how it won't hide the original.
- Add opt-r shortcut for quickly rebasing selected commit onto the current stack base
- Scroll dropdown menus that would have gone off screen, like the settings dropdown
- Allow collapsing the list of queued commands, and truncate extremely long lists

## 0.1.56

### Aug 26 2024

- Horizontally grow the quick commit title input as you type
- Improve reliability of process exit messages, which could sometimes cause incorrect state
- Fix VS Code extension host restarts disconnecting ISL. ISL now restarts on extension host restart
- Add quick button to mount additional workspace folders from the ISL cwd dropdown
- Ensure files are sorted the same in the comparison view as in the changed files list
- Add VS Code config option to open files / diffs / comparisons beside the ISL window instead of re-using the same view column
- Add file status name to the tooltip when hovering on a file
- Remote "Beta" label on chunk selection UI

## 0.1.55

### Aug 13 2024

- Improve error messages from `sl` to show the actual issue in the error UI
- Allow selecting text from error messages without closing the error message
- Truncate long lists of bookmarks
- Fix focus getting lost when tabbing back and forth to ISL in VS Code
- Make it possible to open the Comparison View in its own separate panel
- Use configured VS Code font size and ligatures in the comparison view
- Fix errors when trying to discard many files with some unchecked

## 0.1.54

### Jul 25 2024

- Allow editing empty commit titles in interactive split
- Increase padding between stacks
- Fix repos without Merge Queue support not being able to fetch diff info (Thanks to [@alex-statsig](https://github.com/alex-statsig))
- Fix top bar visually jumping when loading new data at certain screen sizes
- Fix button text unintentionally wrapping
- Fix the commit info sidebar being too large and having buttons go offscreen
- Fix file paths getting stuck in full path mode when using a VS Code shortcut that includes opt/ctrl.

## 0.1.53

### Jul 8 2024

- Handle acting on optimistic commits without errors
- Improve how long arguments to commands are rendered

## 0.1.52

### Jun 24 2024

- Fix extra spaces when typing in a TextField
- Made styling of some components more consistent
- Don't consider Untitled files as unsaved
- Fix split confirmation buttons to the top so they can always be clicked on small screens
- Added ability to create a bookmark from the context menu
- Fix a crash when selecting an optimistic commit in some cases
- Made the size of the description in the Commit Info sidebar more consistent

## 0.1.51

### Jun 3 2024

- Fix fields in the commit info view sometimes not focusing when you start editing them
- Pressing the spacebar while a commit field is focused will now start editing the field
- Add a config to turn off condensing stacks of obsolete commits
- Show unsaved files under uncommitted changes, with actions to save all
- When committing / amending, warn if there are unsaved files that are part of the repo, with option to save all
- Add "only fill empty fields" option when filling a commit message from a previous commit, and make this the default
- "Open all files" now won't open generated files by default, unless all changes are to generated files
- "Open all files" when `workbench.editor.enablePreview` is true now skips using preview mode so more than one file is opened
- Show number of selected changes that will be amended / commit, like "2/3"
- Add context menu option to browse your repo at a given public commit, if enabled by the `fbcodereview.code-browser-url` config
- Similarly, add context menu action to copy a file's url when right clicking on a file, if the `fbcodereview.code-browser-url` config is set up
- Improve colors in high contrast themes (notably lines connecting to "You are here")
- Fix commit info view fields not always tokenizing the last token

## 0.1.50

### May 15 2024

- Auto-mark files with conflicts as resolved when saving them
- Show cwds as relative paths from their repo repository
- Automatically run custom configured merge tools instead of requiring a button press
- Fix issue where changing available workspace folders in vscode doesn't update the available cwds in ISL
- Show Split and Edit Stack modals immediately with a loading spinner
- Delay loading Split and Edit Stack data until running commands have finished, to prevent stale data
- Add selection checkboxes when viewing uncommitted changes in "tree" mode
- Add button to clear out the current commit message in the commit info view
- Reduce truncation of long bookmarks
- Fix left/right arrows in interactive split sometimes not moving all selected lines

## 0.1.49

### May 2 2024

- Updates to merge conflict handling
  - Conflicts in deleted files can now be either deleted or marked as resolved
  - Conflicts in deleted files are shown more clearly as being deleted, and why
  - Show the commit being rebased on top of the destination, to make it easier to understand
    - This feature will require a new version of Sapling to work
  - Made labels more consistent, now use the terms "Source - being rebased" and "Dest - rebasing onto" consistently
  - Support for external merge tools, if configured. See `sl help config.merge-tools` for more information.
  - Automatically run merge drivers before continuing a rebase
  - Make Continue / Abort conflict buttons more prominent
- Quickly change your cwd via a dropdown button for the Repository Info & cwd dropdown. You can still open the menu for information.
- Increase the drag target on the right side of commits so you can more easily drag and drop rebase
- Purge added files when partially discaring, making discard more consistent
- Handle commits with no titles, but also prevent them from being created by split
- Merge driver output with `\r` is rendered better in command output

## 0.1.48

### Apr 19 2024

- Fix wrong avatar briefly showing when making a new commit
- Throttle Watchman subscription if it's firing too often
- Only subscribe to Watchman while ISL is open
- Show "Follower" on commits marked as followers via `sl pr follow` (Thanks to [@rejc2](https://github.com/rejc2)!)

## 0.1.47

### Apr 11 2024

- Allow deleting bookmarks via context menu
- Allow scrolling the list of shelves

## 0.1.46

### Apr 5 2024

- Fix GitHub integrations not working
- Syntax highlighting runs in a WebWorker so it doesn't slow down the UI
  - Clicking buttons in Interactive Split can be as much as 10x faster now

## 0.1.45

### Apr 4 2024

- Render comments from GitHub
  - Click the comment icon next to the PR Badge to see comments
  - includes inline comments and suggested changes

## 0.1.44

### Mar 29 2024

- Added Bookmarks manager
  - If you have multiple remote bookmarks, you can control which remote bookmarks are visible

## 0.1.43

### Mar 26 2024

- Fix VS Code diff views sometimes having an empty left side
- Improved behavior of `goto` when downloading commits
- Fixed focus mode not allowing drag and drop rebases outside your stack
- Fixed focus mode showing more commits than intended

## 0.1.42

### Mar 13 2024

- Add "Focus mode" to hide commits other than the current stack
- Add "Apply" button to unshelve without deleting the shelved changes
- Shift click to select ranges of commits now prefers selecting without including branching children
- Remember the collapsed state of generated files
- Show inline spinner next to "you are here" while goto is running
- If commit / amend / amend message hit an error, restore your typed commit message so you can try again
- Clear quick commit title after committing
- Hide generated file content by default in the Split UI
- Ensure commit titles don't shrink too much in compact mode
- Prevent successions from persisting commit message edits to different diffs
- Elided obsolete commits will now be shown if selected
- Fix last run command showing '?' 5 seconds after exiting
- Fix issues when viewing commits after writing a commit message
- Fix "Fold down" button in "Edit Stack" not working

## 0.1.41

### Mar 4 2024

- Fix UI not refreshing after finishing queued commands
- Add option to use "unified" diff view mode for comparison view
  - By default, it uses "split" diff view on wide screens, and "unified" on small screens
- Improve behavior when reconnecting, so commands don't look like they're stuck running
- Some rendering improvements to the comparison view

## 0.1.40

### Feb 26 2024

- Fix persisted state not loading correctly
- Some visual fixes for new commit rendering
- Improve keyboard shortcuts on windows by using ctrl instead of meta
- Thanks to [@alex-statsig](https://github.com/alex-statsig) for several contributions in this release:
  - Add file decorations ([#717](https://github.com/facebook/sapling/issues/717))
  - Don't auto-close drawer when window isn't loaded ([#768](https://github.com/facebook/sapling/issues/768))
  - Add "open in code review" context menu action ([#816](https://github.com/facebook/sapling/issues/816))
  - Show blame by default ([#817](https://github.com/facebook/sapling/issues/817))
  - Fix commit template loading ([#821](https://github.com/facebook/sapling/issues/821))

## 0.1.39

### Feb 21 2024

- Copy rich links to Diffs instead of plain text
- Better error message when no folders are mounted yet
- Hide drawers when ISL isn't loaded, to avoid showing a spinner forever
- Detect some files as generated via regex, such as Cargo.lock files. This regex is configurable.
- Show error notification if opening a file fails
- Remove "undefined" in tooltip for files
- Allow specifying a custom command to open files (outside VS Code)
- Fix some state not persisting, such as drawer collapsed state
- Fix dragging commits in edit stack being misaligned with the cursor

## 0.1.38

### Feb 9 2024

- Fix vscode extension not properly loading

## 0.1.37

### Feb 7 2024

- Fill blank commit messages from previous commits
- Context menu option to rebase a commit
- Close other dropdowns when opening a menu from the top bar
- Fix opening non-text files like images

## 0.1.36

### Jan 30 2024

- Fixed an issue where Pull button and cwd selector didn't appear in some cases
- Updated tooltips for download menu and commit mode selector

## 0.1.35

### Jan 25 2024

- Reduced polling frequency when ISL not visible
- Remove arrow from "diff" icon
- Prevent acting on obsolete commits to prevent confusing commit duplication
- Set to amend mode when opening a commit in the sidebar
- Make list of commits to submit scrollable
- Show a confirmation toast when copying hashes and other data to the clipboard
- Improve "You are here" and commit selection in high-contrast themes
- Updated Goto tooltip
- Improvements to split UI tracking copied files
- Make font sizes more consistent with ISL outside of vscode
- Experimental DAG-based renderer, hidden behind an SL config `isl.experimental-graph-renderer=1`

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
