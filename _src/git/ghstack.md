---
sidebar_position: 3
---

import {Command, SLCommand} from '@site/elements'

# ghstack

Sapling includes a custom version of [`ghstack`](https://github.com/ezyang/ghstack) via its builtin <SLCommand name="ghstack" /> subcommand.

:::caution

1. `ghstack` requires having _write_ access to the GitHub repo that you cloned. If you do not have write access, consider using [Sapling Stacks](./sapling-stack.md) instead.
2. You will NOT be able to merge these pull requests using the normal GitHub UI, as their base branches will not be `main` (or whatever the default branch of your repository is). Instead, lands must be done via the command line: `sl ghstack land $PR_URL`.

:::

One important difference between Sapling's version of ghstack is that **it does not rely on a `~/.ghstackrc` file**. So long as you have configured the GitHub CLI as described in [Getting Started](../introduction/getting-started.md#authenticating-with-github), you can start using `sl ghstack` directly.

Once you have a stack of commits, you can use `sl ghstack` to create a pull request for each commit in the stack, or to update existing pull requests linked to the commits.

See the help text for the <Command name="ghstack" /> command for more details.
