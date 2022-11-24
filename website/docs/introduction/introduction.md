---
sidebar_position: 0
---

# Sapling SCM

Sapling is a source control system developed and used at Meta that places special emphasis on usability and scalability. Git and Mercurial users will find many of the basic concepts familiar, and that workflows like understanding your repository, working with stacks of commits, and recovering from mistakes, are substantially easier.

When used in conjunction with the Sapling server and virtual filesystem (not yet available publicly), Sapling scales to repositories with 10’s of millions of files, commits, and branches. While inside Meta it is primarily used for our large monorepo, the Sapling CLI also supports cloning Git repositories and can be used by individual developers to work with GitHub, etc.

### Why make a new version control?

Sapling began 10 years ago as an effort to make Meta’s monorepo scale in the face of ever increasing engineering growth.  Publicly available source control systems were not, and still are not, capable of handling repositories of this size. Instead of moving away from the monorepo and sacrificing engineering velocity, we decided to build the source control system we needed.

Along the way we realized there were also large opportunities to increase developer velocity by improving the source control experience. By investing in new UX, we’ve made it possible for even new engineers to understand their repository and do things that were previously only possible for power users.

Additionally, as we developed Sapling we ended up with internal abstractions that happened to make it straightforward for us to add Git support. This idea that the UX and scale of your version control could be separated from the repository format has allowed us to, in effect, have our cake and eat it too by letting us use our scalable system internally, and still interact with Git repositories where needed.  We hope that this pattern might also provide an example path for how source control could evolve beyond the current industry wide status quo.

### Basic concepts

The easiest way to understand the basic usage of Sapling is to see it in action. Below we clone a repo, make some commits/amends, undo some changes, and push the work.

```sl-shell-example
# Clones the repository into the sapling directory.
# For git support, it uses git under the hood for clone/push/pull.
$ sl clone --git https://github.com/facebookexperimental/sapling
remote: Enumerating objects: 639488, done.
...
$ cd sapling

# 'sl' with no arguments prints the smartlog.
# It shows the commit you are on (@) and all of your local commits
# (none in this example since we just cloned). For each local commit it shows
# the short hash, the commit date, who made it, any remote bookmarks,
# and the commit title. With some configuration it can also show
# information from inside the commit message, like task numbers or
# pull request numbers.
$ sl
@  c448e50fe  Today at 11:06  aaron  remote/main
│  Use cached values
~

# Checkout a commit in main that I want to debug.
# The dashed line in smartlog indicates we're not showing some commits
# between main and my checked out commit.
$ sl goto a555d064c
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷
@  a555d064c  Today at 09:06  jordan
│  workingcopy: Give Rust status exclusive ownership of TreeState
~

$ vim broken_file.rs
$ vim new_file.txt

# 'sl status' shows which files have pending changes. 'sl st' also works.
# 'M' indicates the file is modified. '?' indicates it is present but not tracked.
$ sl status
M broken_file.rs
? new_file.txt

# 'sl add' marks the untracked file as new and tracked.
# It will now show up as 'A' in status.
$ sl add new_file.txt

$ sl commit -m "Fix bug"
$ vim broken_file.rs
$ sl commit -m "Add test"
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ @  13811857c  1 second ago  mary
╷ │  Add test
╷ │
╷ o  95e6c6b86  10 seconds ago  mary
╭─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~

# Go to the previous commit.
$ sl prev
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ o  13811857c  21 seconds ago  mary
╷ │  Add test
╷ │
╷ @  95e6c6b86  30 seconds ago  mary
╭─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~

# Amend the first commit
$ vim build.sh
$ sl amend
95e6c6b863b7 -> 35740664b28a "Fix bug"
automatically restacking children!
rebasing 13811857cc1e "Add test"
13811857cc1e -> d9368dec77e1 "Add test"

# Note how the stack remained together, despite editing the first commit.
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ o  d9368dec7  81 seconds ago  mary
╷ │  Add test
╷ │
╷ @  35740664b  17 seconds ago  mary
╭─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~

# You can optionally create a local bookmark if you want.
$ sl bookmark my_task
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ o  d9368dec7  107 seconds ago  mary
╷ │  Add test
╷ │
╷ @  35740664b  43 seconds ago  mary  my_task*
╭─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~

# Abandon the second commit.
$ sl hide -r d9368dec7
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ @  35740664b  68 seconds ago  mary  my_task*
╭─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~

# Let's bring back the original version of the first commit.
# Note how smartlog marks the original commit as obsolete ('x')
# and explains how 95e6c6b86 was amended to become 35740664b.
$ sl unhide -r 95e6c6b86
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ @  35740664b  110 seconds ago  mary  my_task*
╭─╯  Fix bug
│
│ x  95e6c6b86 [Amended as 35740664b28a]  3 minutes ago  mary
├─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~

# Rollback the amend we did earlier.
$ sl unamend
$ sl
o  c448e50fe  Today at 11:06  remote/main
╷
╷ @  95e6c6b86  4 minutes ago  mary  my_task*
╭─╯  Fix bug
│
o  a555d064c  Today at 09:06
│
~
$ sl status
M build.sh
$ sl revert --all

# Push the commit to the remote main branch.
# Note, when pushing to Git you would have to rebase onto main
# first via 'sl rebase --dest main'. When pushing to a Sapling server,
# the server would perform the rebase for you, as shown here.
$ sl push --to main
$ sl
@  e97a27666  1 minute ago  mary  remote/main
│  Fix bug
~
```

### Caveats

Some noteworthy caveats about Sapling:

* Some of the design decisions are geared towards corporate, always-online, single-master, rebase-instead-of-merge, monorepo environments.  While Sapling is flexible enough to work outside of these constraints, it is most polished and battle tested in that kind of environment.
* Git support may have some remaining kinks to be worked out.

###
