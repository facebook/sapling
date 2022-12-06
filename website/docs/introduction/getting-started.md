---
sidebar_position: 20
---

import {gitHubRepo, gitHubRepoName} from '@site/constants'

import {Command, ReviewStackScreenshot, SLCommand} from '@site/elements'

# Getting started

This section will walk you through cloning your first repo, making commits, and submitting them as GitHub pull requests.

## Setting your identity

Once you have `sl` [installed](./installation.md) on the command line, you should start out by configuring the identity you wish to use when authoring commits:

```
sl config --user ui.username "Alyssa P. Hacker <alyssa@example.com>"
```

If you do not already have a global Sapling config file, the command above will create it for you. The location of the file varies by platform, though you can run `sl configfile --user` to find it.

- Linux `~/.config/sapling/sapling.conf` (or `$XDG_CONFIG_HOME` instead of `~/.config`, if set)
- macOS `~/Library/Preferences/sapling/sapling.conf`
- Windows `%APPDATA%\sapling\sapling.conf`

## Authenticating with GitHub

Sapling has a number of custom integrations with GitHub pull requests. In order to communicate with GitHub, Sapling needs a [personal access token](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/creating-a-personal-access-token) to talk to the GitHub API. Rather than have Sapling manage your token, we recommend [installing the GitHub CLI (`gh`)](https://cli.github.com/) and using it to authenticate with GitHub as follows:

```
gh auth login --git-protocol https
```

Note that the GitHub CLI is also GitHub's recommended solution for [caching your GitHub credentials in Git](https://docs.github.com/en/get-started/getting-started-with-git/caching-your-github-credentials-in-git) so that you don't have to enter a password or token every time you `sl push`.

## Cloning your first repo

Assuming you authenticated with `gh` using `--git-protocol https`, make sure to be consistent and use the HTTPS URL (as opposed to the SSH URI) for your GitHub repo as an argument to `sl clone`:

<pre>{`\
$ sl clone ${gitHubRepo}
$ cd ${gitHubRepoName}
$ sl
@  fafe18a24  23 minutes ago  ricglz  remote/main
│  migrate packer to new CLI framework
~
`}
</pre>

From inside a repo, running `sl` with no arguments shows you your commit graph. Initially, this will contain only the head of the default branch, `main`.

## Creating your first commit

Sapling provides familiar `add` and `commit`/`ci` commands to create a commit:

```sl-shell-example
$ touch hello.txt
$ sl add .
$ echo 'Hello, World!' > hello.txt
$ sl commit -m 'my first commit with Sapling'
$ sl
  @  5a7b44286  25 seconds ago  alyssa
╭─╯  my first commit with Sapling
│
o  fafe18a24  27 minutes ago  remote/main
│
~
```

Note that unlike Git, there was no need to explicitly declare a new branch before creating a commit. Sapling tracks heads automatically, which are readily visible when you run `sl`.

Another important difference from Git is that _there is no index where changes must be staged for commit_. If you had run the above commands using `git` instead of `sl`, the Git commit would contain an empty `hello.txt` file with the non-empty version of the file waiting to be staged.

## Creating your first stack

For illustration purposes, we'll go ahead and create a few more commits:

```sl-shell-example
$ echo foo > foo.txt ; sl add foo.txt ; sl ci -m 'adding foo'
$ echo bar > bar.txt ; sl add bar.txt ; sl ci -m 'adding bar'
$ echo baz > baz.txt ; sl add baz.txt ; sl ci -m 'adding baz'
$ sl
  @  4d9180fd8  1 second ago  alyssa
  │  adding baz
  │
  o  3cc43c835  7 seconds ago  alyssa
  │  adding bar
  │
  o  4f1243a8b  11 seconds ago  alyssa
╭─╯  adding foo
│
o  f22585511  Oct 06 at 17:40  remote/main
│
~
```

After creating your stack, `sl` uses `@` to indicate that you are at the top of the stack of commits you just created. The <Command name="go" /> command supports a number of special aliases, such as <Command name="go" linkText="sl go top" /> and <Command name="go" linkText="sl go bottom" /> to navigate to the top and bottom of your stack, respectively.

You can also use the <Command name="next" /> and <Command name="prev" /> commands to move up and down the stack. Both of these commands take an optional number of "steps" to take, e.g., <Command name="next" linkText="sl next 2" /> will move two commits up the stack.

## Manipulating your stack

See [Basic Commands](../overview/basic-commands.md) to learn more about manipulating your stack from the command line.

You may also want to try Sapling's built-in GUI that runs in the browser . Run <SLCommand name="web" /> to launch it from the command line:

```sl-shell-example
$ sl web
Listening on http://localhost:3011/?token=929fa2b3d75aa4330e0b7b0a10822ee0&cwd=%2FUsers%2Falyssa%2Fsrc%2Fsapling
Server logs will be written to /var/folders/5c/f3nk25tn7gd7nds59hy_nj7r0000gn/T/isl-server-logKktwaj/isl-server.log
```

Sapling will open the URL automatically in your browser. See the docs on [Interactive Smartlog](../addons/isl.md) to learn more about its many features. Interactive Smartlog is also available in our [VS Code Extension](../addons/vscode).

## Submitting pull requests

Sapling supports multiple workflows for interacting with GitHub pull requests. The simplest solution is the <SLCommand name="pr" /> command:

```sl-shell-example
$ sl pr
...
$ sl
  @  4d9180fd8  6 minutes ago  alyssa  #178
  │  adding baz
  │
  o  3cc43c835  6 minutes ago  alyssa  #177
  │  adding bar
  │
  o  4f1243a8b  6 minutes ago  alyssa  #176
╭─╯  adding foo
│
o  f22585511  Oct 06 at 17:40  remote/main
│
~
$ sl pr
#178 is up-to-date
#177 is up-to-date
#176 is up-to-date
no pull requests to update
```

As shown, running `sl pr` creates a pull request (PR) for every commit in your local stack. Note this creates "overlapping pull requests," which means each PR uses the associated commit as the head of the PR and `remote/main` as the base.  Reviewing overlapping pull requests on GitHub can be confusing, so we also provide [ReviewStack](../addons/reviewstack.md) as an alternative code review tool that handles these kinds of pull requests better.

After you have created an initial series of pull requests using <Command name="pr" sl={true} />, you will likely make local changes to your commits that need to be submitted for review. To publish these local changes to GitHub, simply run <Command name="pr" sl={true} /> again to update your existing PRs. Note if you have introduced new commits in your stack that are not linked to a PR, <Command name="pr" sl={true} /> will create pull requests for those, as well.

The "overlapping pull requests" approach may not be an appropriate solution for your project. To that end, we also support an alternative pull request workflow, <Command name="ghstack" sl={true} />, which avoids the "overlapping pull requests" issue, but may not be an option for all projects. See the [Pull Requests section](../git/intro.md#pull-requests) in **Using Sapling with GitHub** to determine which workflow is right for you.

## Browsing pull requests

If you have used Sapling to create pull requests for your commits, then you can use `sl ssl` to include the pull request status in your Smartlog. Note that `sl ssl` is not a subcommand, but a built-in alias for `sl smartlog -T {ssl}`:

```sl-shell-example
$ sl ssl
  @  4d9180fd8  6 minutes ago  alyssa  #178 Unreviewed
  │  adding baz
  │
  o  3cc43c835  6 minutes ago  alyssa  #177 Approved
  │  adding bar
  │
  o  4f1243a8b  6 minutes ago  alyssa  #176 Closed
╭─╯  adding foo
│
o  f22585511  Oct 06 at 17:40  remote/main
│
~
```

In addition to reviewing pull requests on github.com, you may also want to try [ReviewStack](../addons/reviewstack.md), which is our novel user interface for GitHub pull requests with custom support for _stacked changes_.

To view a GitHub pull request on ReviewStack, take the original URL:

> https://github.com/facebook/react/pull/25506

and replace the `github.com` domain with `reviewstack.dev`:

> https://reviewstack.dev/facebook/react/pull/25506

On ReviewStack, the diff and the timeline for a pull request are displayed side-by-side rather than split across tabs. Read the [ReviewStack docs](../addons/reviewstack.md) to learn more about the various features it offers.

<ReviewStackScreenshot />

By default, pull requests in the Smartlog displayed by `sl` are linked to the corresponding page on `github.com`, but you can run the following to configure the Smartlog to link to `reviewstack.dev` instead:

```
sl config --user github.pull_request_domain reviewstack.dev
```
