---
sidebar_position: 40
---
# Differences from Git

While Sapling is similar to Git in that it is distributed, uses hash-addressed commits, has branches (called bookmarks), and uses a clone/pull/push/commit/rebase model, there are a number of behavioral differences.

The list of differences below is not comprehensive, nor is it meant to be a competitive comparison of Git and Sapling. Rather, it highlights some interesting differences for curious people who are already familiar with Git. Sapling has substantial scaling differences as well, which are not covered here.

#### Sapling does not require or encourage using named local branches.
In Git, your repository is defined by the location of your branches, and you pretty much must be on a local branch whenever you do work. In addition, any amend/rebase you do to one branch has no impact on any other branch.

In Sapling, local bookmarks (the equivalent to a Git branch) are completely optional and generally not even used. You do have "branches" in the sense that you can make a stack of commits that fork off the main line of the commit graph, but there is no need to put a label on it. All of your commits are easily visible in your “smartlog” and can be accessed via its hash.

Instead of deleting a branch to delete commits, you can “hide” and “unhide” commits. Not requiring bookmarks simplifies the mental model of the repo and has generally been well regarded by our users. Local bookmarks may still be used as a convenient label for commits, but note that rebasing a commit will move all local bookmarks along with the commit. Remote bookmarks are still required and are locally immutable, similar to origin/main in Git.

#### Sapling has no staging area.
In Git, you must add changes to the staging area before you commit them. This can be used to commit/amend just part of your changes.

In Sapling, there is no staging area. If you want to commit/amend just part of your changes you can use `commit/amend -i` to interactively choose which changes to commit/amend. Alternatively, you can simulate a staging area by making a temporary commit and amending to it as if it was the staging area, then use `fold` to collapse it into the real commit.

#### Sapling may not download all the repository data during clone/pull.
In Git, a clone or pull will generally fetch all new repository data.

In Sapling, a clone or pull will only fetch the main branches of a repo. Other branches will be fetched on demand. `push` only updates one remote branch. When used with a supported server, Sapling might fetch commit data (messages, date, or even hashes), tree and file data on demand. These avoid downloading unnecessary data, at the cost of requiring the user to be online more often.

#### Sapling has first-class support for undo commands.
In Git, to undo many operations requires a deeper understanding of the Git commit model and how git checkout/reset/reflog interact with that model.

In Sapling, there are `uncommit`, `unamend`, `unhide`, and `undo` commands for undoing common operations.  Additionally, `undo -i` allows you to go back across multiple operations and gives a visual preview of the post-undo state of the repository before you do it.

#### Sapling does not use `rebase -i` for editing stacked commits.
In Git, when working with a stack of commits, you are generally required to use `git rebase -i` to edit commits in the middle of the stack, which is a notably complex flow.

In Sapling, when working with a stack of commits you can just checkout the commit you want to work on and run “amend”, “split”, “fold”, etc to modify the middle of the stack. The top of the stack is automatically kept track of and restacked for you so your stack remains together. Additionally, `absorb` allows automatically sucking pending changes down into the appropriate commit in your stack. `histedit` can be used to provide a `rebase -i` like experience if desired.

#### Sapling generally does one thing per command.
In Git, a command may do multiple seemingly unrelated things. `checkout` may be used to move to another commit, revert the contents of an individual file, and create a branch. `reset` may be used to move a branch and undo certain operations. `rebase` can be used to move commits or edit a stack.

In Sapling, each command generally does one thing. `pull` fetches remote commits without merging. `goto` moves you to another commit. `revert` adjusts the contents of files in the working copy. `bookmark` create a bookmark. `rebase` moves commits, etc.

#### Sapling allows pushing “onto” a bookmark (when used with a Sapling compatible server).
In Git, pushing involves sending your commits to the server and updating the server branches to point at the new commits.

In Sapling, when used with a Sapling compatible server, a push sends your commits to the server then the server rebases your commit onto the target bookmark (as long as the rebase would not require merging file changes) and moves the target bookmark forward.  This allows many pushes to succeed at once, without requiring people to pull-then-rebase-then-push again to win a push race.

#### Sapling supports “sparse profiles” for sharing sparse configuration (when not using the Sapling virtual filesystem).
In Git, users are responsible for manually managing their own sparse configuration.

In Sapling, sparse configuration can be checked into the repo as a "sparse profile" file which lists all the paths to include/exclude. This allows all users on a team or in an org to use the same sparse profile. As dependencies change, the shared profile can be updated so that everyone always has the correct files without every engineer having to update their setup.

#### Sapling tracks the history of a commit as it’s changed over time.
In Git, if you amend or rebase a file, there is no record that the new version of the commit came from the old version.

In Sapling, when you amend, rebase, fold, split, etc a record of the operation is kept and you can view the mutation-history of that commit. This history is used to automate certain rebases for you. For instance, if you have a stack of five commits and the first commit gets rebased and pushed to `main` by your CI system, Sapling will know that your local commit #1 became commit X in `main` and can automatically rebase commits 2-5 onto the new `main` version. This becomes particularly powerful when working across multiple machines or with multiple people on a stack, as it allows the stack to stay together even as different people/machines edit different parts of it.
