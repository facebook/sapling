# Changelog

## 0.1.18

- Allow pressing Enter to quick commit
- Add button to open a file in the comparison view
- Add tooltip to copy filenames in the comparison view
- Fix white line artifact when selecting a commit
- Fix PR links in blame hover

## 0.1.17

- Add dropdown to pull a specific commit from remote
- Experimental partial commit UI hidden behind `isl.experimental-features` config
- Thanks to [@alex-statsig](https://github.com/alex-statsig) for several contributions in this release:
  - [Experimental inline blame annotations, disabled in settings by default](https://github.com/facebook/sapling/pull/640)
  - [Fix diff views being backwards](https://github.com/facebook/sapling/pull/637)
  - [Fix missing data until first poll](https://github.com/facebook/sapling/pull/638)
  - [Fix github CI status check](https://github.com/facebook/sapling/pull/651)

## 0.1.16

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

- Add repo selector if multiple workspace folders are mounted
- Add "View Changes" context menu action on commits to quickly diff their changes
- Show diff badges inline on large displays to better use horizontal space
- Experimental stack editing UX hidden behind `isl.experimental-features` `sl` config

## 0.1.14

- Fix tooltips not disappearing (such as on pull button)
- Use normal font-smoothing for more readable text
- Hide uncommit button on closed PRs

## 0.1.13

- Customize how changed file paths are displayed: minimal, full file path, tree view, or fish-shell-style
- Copy quick commit form title into full commit form when clicking "Commit as..."
- Fix tooltips wrapping text mid-word
- Allow repos cloned without http prefix

## 0.1.12

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

- Added revert button to VS Code SCM Sidebar files
- Added button to open diff view for VS Code SCM Sidebar files
- Use --addremove flag when committing/amending so untracked files are included
- Fix ssh:// upstream paths for GitHub repos not being detected as valid repos
- Better styling of Load More button and commit graph

## 0.1.9

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

- ISL no longer crashes when a language other than English is selected in VS Code: <https://github.com/facebook/sapling/issues/362>.
- Added an ISL menu button to the source control panel: <https://github.com/facebook/sapling/commit/538c6fba11ddfdae9de93bf77cffa688b13458c0>.
- Updated the Sapling icon: <https://github.com/facebook/sapling/commit/2f7873e32208d4cd153b7c1c1e27afe19e815cf0>.

## 0.1.7

- Fixed an issue where we were stripping the trailing newline in the output to `sl cat`, which caused the VS Code extension to constantly report that the user had modified a file by adding a newline to the end: <https://github.com/facebook/sapling/commit/f65f499ba95a742444b61cb181adb39d2a3af4c2>.

## 0.1.6

- Fixed an issue with path normalization that was preventing extension commands from working on Windows because files were not recognized as part of a Sapling repository: <https://github.com/facebook/sapling/commit/206c7fbf6bc94e7e5940630b812fba7dcd55140e>.
- Cleaned up the instructions on how to use the extension in the README: <https://github.com/facebook/sapling/commit/4ee418ca7aab519b1b4f96edd0991311e8c6b03f>
- Fixed an issue where the **See installation docs** button in ISL failed to open the installation docs: <https://github.com/facebook/sapling/issues/282>.

## 0.1.5

- Did not realize a release and pre-release cannot share a version number. Re-publishing the 0.1.4 pre-release with 4c29208c91256f4306aec9f0e9ec626e96ea3cba included as an official release.

## 0.1.4

- Fixed #282: Add config option to set what `sl` command to use
- More reliably detect command not found on Windows

## 0.1.3

- Support GitHub enterprise and non-GitHub repos in ISL
- Add revert button next to uncommitted changes in ISL
- Add repo/cwd indicator at the top of ISL
- Show a spinner while the comparison view is loading
- Fix tooltips being misaligned in corners
- Make styling more consistent between web and VS Code

## 0.1.2

- Fix the comparison view not scrolling
- Show an error in ISL if Sapling is not yet installed

## 0.1.1 - Initial release

Features:

- Interactive Smartlog (ISL) embedded as a webview
- Simple support for VS Code SCM API, including showing changed files
- Diff gutters in changed files
- VS Code Commands to open diff views for the current file
