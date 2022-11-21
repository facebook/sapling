---
sidebar_position: 10
---

import {Video, Command, SLCommand, ThemedImage} from '@site/elements'

# Interactive Smartlog (ISL)

Interactive Smartlog (ISL) is a web-based, graphical interface for working with your Sapling smartlog. It is available as part of Sapling Web, which you can launch from the command line as follows:

```
sl web
```

By default, this will start the Sapling Web server and open the UI in a local browser.

See the docs for the <SLCommand name="web" /> command for the full set of options.

<ThemedImage alt="ISL Overview" light="/img/isl/isl_overview_light.png" dark="/img/isl/isl_overview_dark.png" />


## What can you do with ISL?

As the name suggests, Interactive Smartlog is an _interactive_ form of the _smartlog_ command.
ISL shows you a tree of your commits, with each commit including information about Pull Requests, bookmarks, and more.
Rather than typing commit hashes, you can directly click on commits to interact with them.
For example, rebasing a commit is as simple as dragging and dropping.

While ISL doesn't provide every single feature of the Sapling CLI, it is designed to
simplify everyday workflows and provide an extremely clear picture of your local changes,
which is often all that's needed.


### Working with commits and stacks
The main commit tree in ISL has an indicator that says **You are here**, showing
which commit you are currently on.
You can go to different commits by hovering on them and clicking `Goto` to run <Command name="goto" linkText="sl goto" />.

<ThemedImage alt="Go to commits" light="/img/isl/goto_light.png" dark="/img/isl/goto_dark.png" />

As you create new commits, they will be created on top of each other, forming _stacks_.
This is similar to branches in Git.
A commit can also have more than one commit stacked on top of it.


You can drag and drop commits to rebase them. This is the easiest way to re-arrange commits and manipulate stacks.

<Video src="/img/isl/drag_and_drop_rebase_light.mov" />


Note that drag-and-drop rebasing is not allowed while you have uncommitted changes, since it's harder to deal with merge conflicts.
Commit any uncommitted changes first to work around this.

Drag-and-drop performs a <Command name="rebase" linkText="sl rebase" />, including all commits stacked on top of the commit being dragged. If you want to re-arrange commits within your stack, consider using [`sl histedit`](../commands/histedit.md).


### Running commands
Buttons in ISL run Sapling commands for you.
For example, there is a <Command name="pull" linkText="Pull" /> button at the top left to pull the latest changes from upstream.

While a command is running, you will see progress information at the bottom of the screen.
This is also where you can see error messages if something goes wrong when running a command.
ISL shows the arguments used to run commands, so you could replicate the behavior on the CLI if you want to.

<ThemedImage alt="Command Progress" light="/img/isl/command_progress_light.png" dark="/img/isl/command_progress_dark.png" />

Some commands like <SLCommand name="status" /> will run automatically in the background to fetch data so the UI is always up to date.

Commands will automatically queue up to be run as you interact with the UI. ISL allows you to continue to perform additional actions
while previous commands are running or queued up. This is kind of like chaining together commands on the CLI: `sl pull && sl rebase main && sl goto main`.
Similar to `&&` on the CLI, if any command along the way fails or hits merge conflicts, all further queued commands will be cancelled.

### Making commits and amending

Changes to files in your working copy appear automatically in ISL,
just like if you had run <SLCommand name="status" />.
The color and icon next to files shows you if a file was modified, added, or removed. You can click on files to open them in your Operating System's
default program for that file type.

<ThemedImage alt="Uncommitted Changes" light="/img/isl/uncommitted_changes_light.png" dark="/img/isl/uncommitted_changes_dark.png" />

Underneath your uncommitted changes, there's a **Commit** button and an **Amend** button.
**Commit** will create a new commit out of your changes.
**Amend** will update the previous commit with your newest changes.

When hovering on these buttons, you'll see there's also additional **Commit as...** and **Amend as...** buttons to first write or update
the commit message before running commit/amend. Clicking these buttons opens up the commit form sidebar on the right side,
where you can write a detailed commit message. When you're satisfied with your message, the _Commit_ and _Amend_ buttons at the bottom right will
let you create or amend your commit using your message.

<ThemedImage alt="Commit Form" light="/img/isl/commit_as_light.png" dark="/img/isl/commit_as_dark.png" />


### Interacting with code review

:::tip

In order to interact with GitHub for code review in ISL, be sure to install the `gh` GitHub CLI. [Learn more.](../git/intro.md)

:::


ISL considers code review an integral part of the source control workflow. When making commits, you usually want to submit it for review.
In the commit form on the right, ISL has a button to _Commit and Submit_, as well as _Amend and Submit_.

These will run a submit command on your stack of commits to submit them for code review on GitHub.

You have two options for which command to use to submit for GitHub, <SLCommand name="ghstack" /> and <SLCommand name="pr" />.
ISL will prompt you for your choice the first time you try to submit. This can also be controlled by setting `github.preferred_submit_command` to `ghstack` or `pr`:
```
sl config --local github.preferred_submit_command <ghstack or pr>
```

See documentation on [Using Sapling with GitHub](../git/intro.md) for more information.

<ThemedImage alt="Pull Request Badges" light="/img/isl/pr_light.png" dark="/img/isl/pr_dark.png" />

Commits in your tree which are associated with a GitHub Pull Request will show a badge underneath showing the status of that Pull Request.
You can click this badge to open the Pull Request in GitHub (or [configure it to open alternate domains](../introduction/getting-started#browsing-pull-requests)).

This badge also shows the CI build status and how many comments there are.



### Resolving merge conflicts
Running some commands like <SLCommand name="rebase" /> can sometimes lead to merge conflicts. When merge conflicts are detected, ISL will
change the list of uncommitted changes into a list of unresolved conflicts.

<ThemedImage alt="Merge Conflicts" light="/img/isl/conflicts_light.png" dark="/img/isl/conflicts_dark.png" />

After opening each file and resolving the conflict markers,
you can click the checkmark next to each file in ISL to mark it as resolved.
When all files have been resolved, you are free to continue the command that led to conflicts.

It is possible to hit merge conflicts multiple times, for example, when rebasing an entire stack of commits, as each commit is checked for conflicts one-by-one.

<ThemedImage alt="Resolved Merge Conflicts" light="/img/isl/conflicts_resolved_light.png" dark="/img/isl/conflicts_resolved_dark.png" />


### Comparing changes
ISL includes a comparison view to quickly see all your changes, similar to  <SLCommand name="diff" />
One common use case is to look over all your uncommitted local changes before you submit them for code review.

Just above your uncommitted changes, there's a `View Changes` button to open the comparison view in Uncommitted Changes mode.
In the comparison view, you'll see a split diff view of each file you've changed. You can also access this view with the shortcut `Command+'`.

<ThemedImage alt="Comparison View" light="/img/isl/comparison_light.png" dark="/img/isl/comparison_dark.png" />

The comparison view supports other comparisons as well.
- **Uncommitted Changes**: As mentioned, shows changes to your working copy that haven't been committed or amended yet. This is all the changes of the files `sl status` shows by default. Shortcut: `Command+'`.
- **Head Changes**: Shows all the changes in the current commit, plus any uncommitted changes on top of that. Useful to see what the most recent commit will look like after amending. Shortcut: `Command+Shift+'`
- **Stack Changes**: Shows all the changes in your stack of commits going back to the main branch, plus any uncommitted changes. Useful to see absolutely everything you've changed.
- **Committed Changes**: Shows the changes in a specific commit. This is accessible by selecting a commit then clicking on "View Changes in &lt;hash&gt;". Unlike the other comparisons, this does not include your uncommitted changes.

The comparison view is currently *read-only*.


## Speeding up change detection with Watchman
In order to detect when files have changed in your repository, ISL must occasionally run `sl` commands to check for changes.
To reduce resource usage and speed up how quickly changes are detected, ISL can optionally use [Watchman](https://facebook.github.io/watchman/), a file watching service.
If Watchman is installed on your path, it will automatically be used.
Note that your repository must also have a [`.watchmanconfig`](https://facebook.github.io/watchman/docs/config.html) in the root directory to make use of this feature.


## Connecting to ISL running on another machine

If you are using Sapling on a remote machine, but want to use ISL, you have two options:

### Host with available ports

If you are using Sapling on a remote machine that is able to open ports to the outside world, choose a port like `5000` and pass it as the `-p` argument to `web` when launching it on the remote host:

```
alyssa@example.com:/home/alyssa/sapling$ sl isl --no-open -p 5000
launching web server for Interactive Smartlog...
Listening on http://localhost:5000/?token=a6d646073f28ef2fd09a89bed93e89f4&cwd=%2Fhome%2Falyssa%2Fsapling
Server logs will be written to /dev/shm/tmp/isl-server-logqrqvvN/isl-server.log
```

Assuming your remote hostname is `example.com`, take the URL that <SLCommand name="web" /> printed out and replace `localhost` with the hostname like so:

```
http://example.com:5000/?token=a6d646073f28ef2fd09a89bed93e89f4&cwd=%2Fhome%2Falyssa%2Fsapling
```

You should be able to open this URL in your local browser to access ISL.

### Host with no available ports

If you are running Sapling on a host where you do not have permissions to open ports to the outside world, you may be able to leverage _SSH port forwarding_ to access ISL.

On the server:

```
alyssa@example.com:/home/alyssa/sapling$ sl isl --no-open -p 5000
launching web server for Interactive Smartlog...
Listening on http://localhost:5000/?token=a6d646073f28ef2fd09a89bed93e89f4&cwd=%2Fhome%2Falyssa%2Fsapling
Server logs will be written to /dev/shm/tmp/isl-server-logqrqvvN/isl-server.log
```

On your local machine where you have SSH access to the server:

```
ssh -L 4000:localhost:5000 -N alyssa@example.com
```

Note that this command will stay running in the foreground so long as you remain connected to the remote host. If you lose the connection (perhaps because your computer has gone to sleep), then you will have to run the `ssh` command again to re-establish the connection.

Then take the original URL that you saw on the server and change the port from **`5000`** to **`4000`** before trying to open it on your local machine:

```
http://localhost:4000/?token=a6d646073f28ef2fd09a89bed93e89f4&cwd=%2Fhome%2Falyssa%2Fsapling
```
