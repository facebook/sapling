# Changelog

## 0.1.7

- Fixed an issue where we were stripping the trailing newline in the output to `sl cat`, which caused the VS Code extension to constantly report that the user had modified a file by adding a newline to the end: https://github.com/facebook/sapling/commit/f65f499ba95a742444b61cb181adb39d2a3af4c2.

## 0.1.6

- Fixed an issue with path normalization that was preventing extension commands from working on Windows because files were not recognized as part of a Sapling repository: https://github.com/facebook/sapling/commit/206c7fbf6bc94e7e5940630b812fba7dcd55140e.
- Cleaned up the instructions on how to use the extension in the README: https://github.com/facebook/sapling/commit/4ee418ca7aab519b1b4f96edd0991311e8c6b03f
- Fixed an issue where the **See installation docs** button in ISL failed to open the installation docs: https://github.com/facebook/sapling/issues/282.

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
