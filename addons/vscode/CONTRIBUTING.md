# Sapling VS Code extension

This folder contains the VS Code extension,
including Sapling SCM provider and embedded ISL.

Note: this file acts the techincal README for the vscode/ folder,
while README.md is the user-facing description of the extension visible in the extension marketplace.

The vscode extension consists of two forms of javascript:

- extension code, running in the vscode extension host process.
  This code uses the vscode API and acts like a node process.
- (ISL) webview code, running in a vscode webview.
  This code cannot use the vscode API, and acts like its running in a browser.

The two are built separately, and communicate via message passing.
Unlike web `sl` in isl-server/proxy, this does not use websockets
but rather VS Code's own message passing system (which still works across remoting).

## Building & Running

Build artifacts live in `./dist`.

**Development**:

`yarn watch-extension` to compile extension code
`yarn watch-webview` to compile webview code

**Production**:

`yarn build-extension` to build production extension code
`yarn build-webview` to build production extension code

**Dogfooding**

You can use a development build of the vscode extension by symlinking into this folder,
since package.json points to `dist/`:

```
ln -s ./vscode ~/.vscode/extensions/meta.sapling-scm-100.0.0-dev
```
