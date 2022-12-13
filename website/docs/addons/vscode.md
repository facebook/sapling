---
sidebar_position: 15
---

import {SLCommand, ThemedImage} from '@site/elements'

# VS Code extension

Sapling provides an extension for [Visual Studio Code](https://code.visualstudio.com/).

[You can download the extension from the Microsoft Extension Marketplace](https://marketplace.visualstudio.com/items?itemName=meta.sapling-scm), or by searching from the extensions tab inside VS Code.

:::caution

Sapling must be installed through the normal installation instructions in order for
the VS Code extension to work properly.

[See installation instructions.](../introduction/installation.md)

:::

<ThemedImage alt="ISL in VS Code" light="/img/isl/vscode_light.png" dark="/img/isl/vscode_dark.png" />


### Embedded interactive smartlog
Access the [Interactive Smartlog (ISL)](./isl.md) interface directly within VS Code,
without needing to launch it with <SLCommand name="web" />.
Just run the **Sapling: Open Interactive Smartlog** command from the [command palette](https://code.visualstudio.com/docs/getstarted/userinterface#_command-palette).

### VS Code source control API

Sapling also implements the VS Code API for source control:
- You can see your uncommitted changes in the **Source Control** sidebar.
- Files you change will have gutters that show what lines of code have changed.
- You can open an editable diff viewer from the command palette with
**Sapling SCM: Open Diff View For Current File**.
