# addons/components/

This directory contains a component library used internally by ISL, in an isolated shareable location.

This library is (visually) heavily based on the [VS Code Webview UI Toolkit](https://github.com/microsoft/vscode-webview-ui-toolkit), but slightly streamlined for react usage and doesn't share any code.
It is visually almost identical, since it was used to migrate away from the toolkit without requiring restyling.

## Usage

This package is currently not published to npm, and must be copied or referenced directly.

There's a simple component explorer available by running `npm run start` in this directory.

Alternatively, search ../isl/ for usage examples.

## Differences from vscode-webivew-ui-toolkit

- requires react & stylex
- does not use HTML Custom Elements / Shadow root
- includes default light and dark theme variable definitions as an optional import. But still uses vscode theme variables when used inside of vscode.
- component APIs are changed to fit react usage a bit more directly
- some component designs are tweaked, like `<Button icon>` has a border and background color by default
- added some components

## Components

Like vscode-webview-ui-toolkit:

- **Badge**: usually used for numbers
- **Button**
- **Checkbox**
- **Dropdown**: `<select>`
- **Divider**: `<hr>`
- **Panels**
- **Radio**
- **LinkButton**: `<a>`
- **Tag**: used for text. Unlike the toolkit, this is not forced to be uppercase
- **TextArea**: `<textarea>` for long-form text
- **TextField**: `<input type="text">` for one line of text

Additional components:

- **Banner**: informational banner
- **ButtonDropdown**: `<button>` with a dropdown to change which action to take
- **ButtonGroup**: add multiple button children to meld them into a side-by-side button
- **ErrorNotice**: Used to show an error message + stack trace
- **Flex**: Simple flexbox containers so you don't need to repeat `display: flex` all over the place
- **Icon**: codicon exposed as a component
- **Kbd**: looks like a keyboard key, used for keyboard shortcuts
- **Subtle**: Simply makes text smaller and lighter, to use as an informational byline that is less intense than normal text
- **ThemedComponentRoot**: Unlike the toolkit, we require this parent component at the root of the tree to provide theme variables. This is also how you set the current theme if being used outside of VS Code.
- **Typeahead**
- **ViewportOverlay**: Another top level wrapper used to add tooltips and modals.
- **Tooltip**: Tooltips that can appear on hover or click
