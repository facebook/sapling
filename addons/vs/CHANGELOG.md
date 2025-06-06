# Changelog

All notable changes to this project will be documented in this file.

## [1.0.0] - 2025-05-16

### Added

- Initial public release of the Interactive Smartlog Visual Studio Extension.
- Integration with [Sapling SCM](https://sapling-scm.com/).
- Interactive Smartlog tool window accessible via **View > Other Windows > Interactive Smartlog**.
- WebView2-based UI for displaying the output of `sl web` inside Visual Studio.
- Support for viewing commits, adding changes, rebasing, merging, and other source control operations from within Visual Studio.
- Command to reload the Interactive Smartlog view (**Tools > Reload ISL**).
- Logging and error handling with user notifications.
- Graceful error pages for missing or unmounted repositories.
- .NET Framework 4.7.2 and Visual Studio 2022 support.
- Context menu commands for the active document:
  - View Inline Diff for Uncommitted Changes: Right-click the active document to view a diff of uncommitted changes.
  - View Inline Diff for Stack Changes: Right-click the active document to view a diff of stack changes.
  - View Inline Diff for Head Changes: Right-click the active document to view a diff of head changes.
  - Revert Uncommitted Changes: Right-click the active document to revert uncommitted

### Changed

- N/A (initial release)

### Fixed

- N/A (initial release)

---

**Note:**
This extension requires Sapling SCM to be installed separately. See the [README](./README.md) for details.
