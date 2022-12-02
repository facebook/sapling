---
sidebar_position: 1
---

# Using Sapling with GitHub

When using Sapling with GitHub, we **strongly** recommend the following:

- Install the [GitHub CLI (`gh`)](https://cli.github.com/), as some of our current GitHub integrations rely on it.
- Using the GitHub CLI, authenticate with GitHub via `gh auth login --git-protocol https`.

In order for Sapling to work with GitHub pull requests on your behalf, you must provide it with a [Personal Access Token (PAT)](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token). While the GitHub CLI is not the only way to obtain a PAT, it is certainly one of the most convenient.

While we ultimately plan to remove the requirement of installing the GitHub CLI to use Sapling with GitHub (though the PAT will still be necessary), you will still need some mechanism to cache your GitHub credentials in Git to enjoy the best experience with Sapling. For example, a graphical interface like [Interactive Smartlog](../addons/isl.md) would be unpleasant to use if it prompted your for a password every time it needed to talk to GitHub. To avoid this, [GitHub recommends two solutions for caching your GitHub credentials](https://docs.github.com/en/get-started/getting-started-with-git/caching-your-github-credentials-in-git), the GitHub CLI being the primary choice.

While GitHub [gives you the option](https://docs.github.com/en/get-started/getting-started-with-git/about-remote-repositories) of cloning a repo with either HTTPS or SSH URLs, HTTPS is generally considered easier to use because HTTPS default port 443 is far less likely to be blocked by a firewall than SSH default port 22.

## Cloning a repo

Once you have authenticated via `gh auth login --git-protocol https`, you should be able to clone any GitHub repository via its HTTPS URL that you have access to using Sapling:

```
sl clone https://github.com/facebook/sapling
```

With the GitHub CLI caching your credentials, you will be able to run commands like `sl ssl` to see the status of any linked pull requests in your Smartlog, as it uses your PAT behind the scenes to query their current state.

## Pull requests

When it comes to working with pull requests from Sapling, you have two options: `sl pr` and `sl ghstack`. Each has its tradeoffs, so you may opt to use a different solution, depending on the scenario:

### `sl pr` (aka "Sapling stack")

See the dedicated [Sapling Stack](./sapling-stack.md) page for more information.

**Pros:**

- Works with any GitHub repo.

**Cons:**

- Creates "overlapping" pull requests that may be confusing to reviewers using the GitHub pull request UI. Reviewers are strongly encouraged to use [ReviewStack](../addons/reviewstack.md) for code review instead of GitHub.

### `sl ghstack` (aka [ghstack](https://github.com/ezyang/ghstack) for Sapling)

See the dedicated [ghstack](./ghstack.md) page for more information.

**Pros:**

- Each generated pull request contains one reviewable commit in GitHub.

**Cons:**

- Can only be used if you have _write_ access to the repository.
- You will NOT be able to merge these pull requests using the normal GitHub UI.

## Troubleshooting

### `could not read Username` error when trying to `git push`

If you see an error like the following:

```
stderr: fatal: could not read Username for 'https://github.com': No such device or address
```

Then you likely need to run [`gh auth setup-git [--hostname HOST]`](https://cli.github.com/manual/gh_auth_setup-git) to configure `gh` as a Git credential helper. This will add the following to your `.gitconfig` (though the host will be different if you used `--hostname` to specify your GitHub Enterprise hostname):

```
[credential "https://github.com"]
    helper =
    helper = !/usr/bin/gh auth git-credential
[credential "https://gist.github.com"]
    helper =
    helper = !/usr/bin/gh auth git-credential
```

See [gh issue #3796](https://github.com/cli/cli/issues/3796) for details
