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

When it comes to working with pull requests from Sapling, you have three options: `sl pr`, and `sl push`. Each has its tradeoffs, so you may opt to use a different solution, depending on the scenario:

### `sl pr` (aka "Sapling stack")

See the dedicated [Sapling Stack](./sapling-stack.md) page for more information.

**Pros:**

- Works with any GitHub repo.

**Cons:**

- Creates "overlapping" pull requests that may be confusing to reviewers using the GitHub pull request UI. Reviewers are strongly encouraged to use [ReviewStack](../addons/reviewstack.md) for code review instead of GitHub.

:::tip

You can use the `pr` revset to automatically pull and checkout GitHub pull request. For example, `sl goto pr123`. See `sl help revsets` for more info.

:::

### `sl push`

The GitHub website provides a way to turn a Git branch into a pull request.
You can use `sl push` to create branches, then use the GitHub website to create
pull requests.

If you have write access to the repo, you can push to a new branch:

```
sl push --to remote/my-new-feature
```

If you don't have write access to the repo, you can [fork](https://docs.github.com/en/get-started/quickstart/fork-a-repo?tool=webui) the repo from the
GitHub website, add your fork as a remote, then push to your fork:

```
sl paths --add my-fork ssh://git@github.com/my-username/sapling.git
sl push --to my-fork/my-new-feature
```

After push, open the repo webpage. You will see GitHub detected the push:

<div style={{margin: '20px 0px'}}>
    <div style={{
        background: '#fff8c5',
        border: '1px solid rgba(212,167,44,0.4)',
        borderRadius: '6px',
        color: '#24292f',
        padding: '20px 16px',
    }}>
        <div style={{display: 'flex' }}>
            <div display="block" style={{flex: 'auto'}}>
                <svg aria-hidden="true" height="16" viewBox="0 0 16 16" version="1.1" width="16" style={{
                    display: 'inline-block',
                    marginRight: '6px',
                    verticalAlign: 'text-bottom',
                }}>
                    <path fill="#9a6700" fill-rule="evenodd" d="M11.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122V6A2.5 2.5 0 0110 8.5H6a1 1 0 00-1 1v1.128a2.251 2.251 0 11-1.5 0V5.372a2.25 2.25 0 111.5 0v1.836A2.492 2.492 0 016 7h4a1 1 0 001-1v-.628A2.25 2.25 0 019.5 3.25zM4.25 12a.75.75 0 100 1.5.75.75 0 000-1.5zM3.5 3.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0z"></path>
                </svg>
                <strong>my-new-feature</strong>
                &nbsp;
                had recent pushes less than a minute ago
            </div>
            <span display="block" style={{
                alignSelf: 'center',
                background: '#2da44e',
                border: '1px solid rgba(27,31,36,0.15)',
                borderRadius: '6px',
                boxShadow: 'rgba(0,45,17,0.2) 0px 1px 0px 0px inset',
                color: '#ffffff',
                fontWeight: 500,
                margin: '-8px -4px -8px 16px',
                padding: '5px 16px',
                verticalAlign: 'middle',
                whiteSpace: 'nowrap',
            }}>
                Compare &amp; pull request
            </span>
        </div>
    </div>
</div>

Click the "Compare & pull request" button to create a pull request.

You can also manually specify a Git branch and create a pull request. Read [Creating a pull request from a fork](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/proposing-changes-to-your-work-with-pull-requests/creating-a-pull-request-from-a-fork) for instructions.

To update an existing pull request, use `sl push -f` to force push to the same
branch.


**Pros:**

- Take control of commits (one or more) the pull request contains more
  explicitly.
- Use GitHub website to edit the pull request summary. It is easy to preview
  and auto-complete.

**Cons:**

- Need a few more clicks on the GitHub webpage.
- Only create a single pull request per branch.


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
