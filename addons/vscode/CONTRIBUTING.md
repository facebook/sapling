# Sapling VS Code extension

This folder contains the VS Code extension,
including the Sapling SCM provider and embedded ISL.

> NOTE: This file acts as the technical `README` for the `vscode/` folder,
> while `README.md` is the user-facing description of the extension visible in the extension marketplace.

The VS Code extension consists of two forms of JavaScript:

- Extension code, running in the VS Code extension host process.
  This code uses the VS Code API and acts like a node process.
- Webview code (of ISL), running in a VS Code webview.
  This code cannot use the VS Code API, and acts like it's running in a browser.

The two are built separately, and communicate via message passing.
Unlike web `sl` in `isl-server/proxy`, this does not use WebSockets
but rather VS Code's own message passing system (which still works across remote connections).

## Building & Running

Build artifacts live in `./dist`.

**Development**

- `yarn watch-extension` to compile extension code
- `yarn watch-webview` to compile webview code

**Production**

- `yarn build-extension` to build production extension code
- `yarn build-webview` to build production webview code

**Dogfooding**

You can use a development build of the VS Code extension by symlinking into this folder,
since `package.json` points to `dist/`:

```sh
ln -s ./vscode ~/.vscode/extensions/meta.sapling-scm-100.0.0-dev
```

**Debugging**

VS Code webview source maps don't load automatically, since we produce separate `.js.map` files
instead of inline source maps. The VS Code webview resource system doesn't seem to load these correctly.

To get source maps in the webview, you need to load them manually:

1. Open ISL in VS Code
2. Open the developer tools from the Help menu
3. Go to the "console" tab
4. Change "top" to the "pending-frame" that corresponds to ISL
5. Open `webview.js` in the "sources" tab (e.g. from a stack trace)
6. Right-click on the file, choose "Add source map..."
7. Enter the full URL to the `.map` file: `file:///path/to/addons/vscode/dist/webview/webview.js.map`

Enjoy your proper stack traces, files, and breakpoints! :D
