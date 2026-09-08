import {Command} from '@site/elements'

# Sapling stack

Sapling comes with a [`pr` subcommand](/docs/commands/pr.md) to help you work with GitHub pull requests.

Once you have a stack of commits, you can use <Command name="pr" linkText="sl pr submit --stack" /> (or `sl pr s -s`) to create a pull request for each commit in the stack, or to update existing pull requests linked to the commits.

:::caution

Make sure you have followed the instructions to [authenticate with GitHub using the GitHub CLI `gh`](/docs/introduction/getting-started.md#authenticating-with-github) before using `sl pr`.

:::

:::caution

By default, `sl pr submit` creates _overlapping_ pull requests where each pull request contains the commit that is intended to be reviewed as part of the pull request as well as all commits below it in the stack. This will not "look right" on GitHub, so collaborators who use this command are encouraged to use [ReviewStack](/docs/addons/reviewstack.md) to review these pull requests, as ReviewStack will present only the commit that is intended to be reviewed for each pull request. Alternatively, see [Pull request workflows](#pull-request-workflows) below for the `stacked` workflow, which uses GitHub's native support for stacked pull requests.

:::

## Pull request workflows

The `github.pr-workflow` config option controls how `sl pr submit` maps a stack of commits onto pull requests:

- `overlap` (default): every pull request targets the repository's default branch, so each pull request contains its commit plus all commits below it in the stack. Best reviewed with [ReviewStack](/docs/addons/reviewstack.md).
- `single`: each pull request contains exactly one commit and targets the head branch of the pull request below it in the stack, so each pull request shows only the diff for its own commit.
- `stacked`: like `single`, but the pull requests are also linked together using GitHub's native [stacked pull requests](https://docs.github.com/en/pull-requests/get-started/about-stacked-prs) feature (public preview), so GitHub renders the stack in the pull request UI and can merge and retarget it from the bottom up.

To enable the native stacked workflow:

```
sl config --user github.pr-workflow stacked
```

Notes on the `stacked` workflow:

- GitHub requires all branches of a stack to be in the same repository, so native stacks are not available when contributing from a fork. Pull requests are still created; they are just not linked into a stack.
- If the stack on GitHub no longer matches your local stack (for example, because you reordered or removed commits, or the stack was modified on GitHub), `sl pr submit` warns without modifying it. Run `sl pr submit --restack` to dissolve the stack on GitHub and recreate it to match your local stack.
- Pull requests created with this workflow omit the "Stack created with Sapling" footer from their descriptions, since GitHub displays the stack natively.

If you get into a funny state, try using `sl pr link` or `sl pr unlink` to add or remove associations between commits and pull requests, as appropriate.
