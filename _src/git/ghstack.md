---
sidebar_position: 3
---

import {Command, SLCommand} from '@site/elements'

# ghstack

ghstack (https://github.com/ezyang/ghstack) is a third-party tool designed to facilitate a stacked diff workflow in GitHub repositories by creating a separate pull request for each commit in a stack. To achieve this, it creates a number of synthetic branches under the hood so that each pull request is scoped to the diff for an individual commit.

Sapling includes a custom version of ghstack via its builtin <SLCommand name="ghstack" /> subcommand. It uses the same branching strategy as stock ghstack, so it is possible to publish a stack in Sapling using <SLCommand name="ghstack" /> and then import it into a Git working tree of the same repository using stock ghstack (or vice versa).

If you are not familiar with ghstack, be aware of the following limitations:

:::caution

1. `sl ghstack` requires having _write_ access to the GitHub repo that you cloned. If you do not have write access, consider using [Sapling Stacks](./sapling-stack.md) instead.
2. You will NOT be able to merge these pull requests using the normal GitHub UI, as their base branches will not be `main` (or whatever the default branch of your repository is). Instead, lands must be done via the command line: `sl ghstack land $PR_URL`.

:::

Further, note that Sapling's version of ghstack takes a different approach to configuration and authorization than stock ghstack. Specifically, **it does not rely on a `~/.ghstackrc` file**. So long as you have configured the GitHub CLI as described in [Getting Started](../introduction/getting-started.md#authenticating-with-github), you can start using <SLCommand name="ghstack" /> directly.

Once you have a stack of commits, you can use <SLCommand name="ghstack" /> to create a pull request for each commit in the stack, or to update existing pull requests linked to the commits.

See the help text for the <Command name="ghstack" /> command for more details.
