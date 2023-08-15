# textmate-lib

This is an attempt to create a reusable Node.js module for working
with a corpus of TextMate grammars and leveraging them to tokenize
source code so it can be syntax highlighted in the browser in a way
that is consistent with VS Code.

As a user of this library, you have to:

- Successfully call `loadWASM()` from `vscode-oniguruma`.
- Provide your own `IRawTheme` (as defined in `vscode-textmate`).
- Provide your own map of scope name to `Grammar` where `Grammar` is an
  interface that determines how to fetch TextMate grammar data (in either
  JSON or plist format).

The specifics of how you satisfy these requirements are likely tied to how
you load static resources in your web application.
