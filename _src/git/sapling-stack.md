---
sidebar_position: 2
---

import {Command} from '@site/elements'

# Sapling stack

Sapling comes with a [`pr` subcommand](../commands/pr.md) to help you work with GitHub pull requests.

Once you have a stack of commits, you can use <Command name="pr" linkText="sl pr submit --stack" /> (or `sl pr s -s`) to create a pull request for each commit in the stack, or to update existing pull requests linked to the commits.

:::caution

Make sure you have followed the instructions to [authenticate with GitHub using the GitHub CLI `gh`](../introduction/getting-started#authenticating-with-github) before using `sl pr`.

:::

:::caution

`sl pr submit` creates _overlapping_ commits where each pull request contains the commit that is intended to be reviewed as part of the pull request as well as all commits below it in the stack. This will not "look right" on GitHub, so collaborators who use this command are encouraged to use [ReviewStack](../addons/reviewstack.md) to review these pull requests, as ReviewStack will present only the commit that is intended to be reviewed for each pull request.

:::

If you get into a funny state, try using `sl pr link` or `sl pr unlink` to add or remove associations between commits and pull requests, as appropriate.
