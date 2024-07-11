# addons/components/

This directory contains a component library used internally by ISL, in an isolated shareable location.

This library is (visually) heavily based on the [VS Code Webview UI Toolkit](https://github.com/microsoft/vscode-webview-ui-toolkit), but slightly streamlined for react usage and doesn't share any code.

## Usage

This package is currently not published to npm, and must be copied or referenced directly.

Meta-Internal: If using fbsource, use `yarn add file:../../relative/path/to/addons/components`. @fb-only

## Differences from vscode-webivew-ui-toolkit

- requires react & stylex
- does not use HTML Custom Elements / Shadow root
- includes default light and dark theme variable definitions as an optional import
- component APIs are changed to fit react usage a bit more directly
- some component designs are tweaked, like `<Button icon>` has a border and background color by default
- added some components

## Components

- Button
- Tag
- Checkbox
- Radio
- ButtonDropdown (`<button>` with a dropdown to change which action to take)
- Dropdown (`<select>`)
- Panels
- LinkButton
- TextField (`<input type="text">` for one line of text)
- TextArea (`<textarea>` for long-form text)
