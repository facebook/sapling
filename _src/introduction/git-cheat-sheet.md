---
sidebar_position: 30
---

# Git cheat sheet

Below is a quick cheat sheet for translating a number of Git commands into equivalent Sapling commands.

You can also use the `sl githelp` command, or `sl git` for short, to automatically translate some git commands into their equivalent Sapling command.

```sl-shell-example
$ sl githelp -- git clone https://github.com/facebook/sapling
sl clone https://github.com/facebook/sapling

$ sl git -- checkout 060f340a9 my_file.txt
sl revert -r 060f340a9 my_file.txt
```

### Cloning, pulling, and pushing

|                    | Git                                       | Sapling                                  |
| ------------------ | ----------------------------------------- | ---------------------------------------- |
| Clone              | `git clone http://github.com/foo my_repo` | `sl clone http://github.com/foo my_repo` |
| Pull               | `git fetch`                               | `sl pull`                                |
| Pull a branch      | `git fetch origin REFSPEC`                | `sl pull -B BRANCH`                      |
| Pull and rebase    | `git pull --rebase`                       | `sl pull --rebase`                       |
| Push to a branch   | `git push HEAD:BRANCH`                    | `sl push --to BRANCH`                    |
| Add a remote       | `git remote add REMOTE URL`               | `sl path --add REMOTE URL`               |
| Pull from a remote | `git fetch REMOTE`                        | `sl pull REMOTE`                         |

Sapling [only](differences-git#sapling-may-not-download-all-the-repository-data-during-clonepull) clones and pulls a subset of remote branches.

### Understanding the repository

|                 | Git                  | Sapling       |
| --------------- | -------------------- | ------------- |
| Your commits    | N/A                  | `sl`          |
| Current history | `git log`            | `sl log`      |
| Edited files    | `git status`         | `sl status`   |
| Current hash    | `git rev-parse HEAD` | `sl whereami` |
| Pending changes | `git diff`           | `sl diff`     |
| Current commit  | `git show`           | `sl show`     |

### Referring to commits

|                               | Git     | Sapling   |
| ----------------------------- | ------- | --------- |
| Current commit                | `HEAD`  | `.`       |
| Parent commit                 | `HEAD^` | `.^`      |
| All local commits             | `N/A`   | `draft()` |
| Commits in branch X but not Y | `Y..X`  | `X % Y`   |

See `sl help revset` for more ways of referencing commits.

### Working with files

|                        | Git                           | Sapling                 |
| ---------------------- | ----------------------------- | ----------------------- |
| Add new file           | `git add FILE`                | `sl add FILE`           |
| Un-add new File        | `git rm --cached FILE`        | `sl forget FILE`        |
| Remove file            | `git rm FILE`                 | `sl rm FILE`            |
| Rename file            | `git mv OLD NEW`              | `sl mv OLD NEW`         |
| Copy file              | `cp OLD NEW`                  | `sl cp OLD NEW`         |
| Add/remove all files   | `git add -A .`                | `sl addremove`          |
| Undo changes           | `git checkout -- FILE`        | `sl revert FILE`        |
| Undo all changes       | `git reset --hard`            | `sl revert --all`       |
| Delete untracked files | `git clean -f`                | `sl clean`              |
| Output file content    | `git cat-file -p COMMIT:FILE` | `sl cat -r COMMIT FILE` |
| Show blame             | `git blame FILE`              | `sl blame FILE`         |

### Working with commits

|                       | Git                                  | Sapling                            |
| --------------------- | ------------------------------------ | ---------------------------------- |
| Commit changes        | `git commit -a`                      | `sl commit`                        |
| Modify commit         | `git commit -a --amend`              | `sl amend`                         |
| Move to commit        | `git checkout COMMIT`                | `sl goto COMMIT`                   |
| Remove current commit | `git reset --hard HEAD`^             | `sl hide .`                        |
| Edit message          | `git commit --amend`                 | `sl metaedit`                      |
| Rebase commits        | `git rebase main`                    | `sl rebase -d main`                |
| Complex rebase        | `git rebase --onto DEST BOTTOM^ TOP` | `sl rebase -d DEST -r BOTTOM::TOP` |
| Rebase all            | N/A                                  | `sl rebase -d main -r 'draft()'`   |
| Interactive rebase    | `git rebase -i`                      | `sl histedit`                      |
| Interactive commit    | `git add -p`                         | `sl commit -i / sl amend -i`       |
| Cherry-pick           | `git cherry-pick COMMIT`             | `sl graft COMMIT`                  |
| Stash changes         | `git stash`                          | `sl shelve`                        |
| Unstash changes       | `git stash pop`                      | `sl unshelve`                      |

### Undo, redo, and reverting

|                              | Git                           | Sapling             |
| ---------------------------- | ----------------------------- | ------------------- |
| Undo commit                  | `git reset --soft HEAD^`      | `sl uncommit`       |
| Undo partial commit          | `git reset --soft HEAD^ FILE` | `sl uncommit FILE`  |
| Undo amend                   | `git reset HEAD@{1}`          | `sl unamend`        |
| Undo rebase/etc              | `git reset --hard HEAD@{1}`   | `sl undo`           |
| Revert already landed commit | `git revert COMMIT`           | `sl backout COMMIT` |
| View recent commits          | `git reflog`                  | `sl journal`        |
| Recover commit               | `git reset COMMIT`            | `sl unhide COMMIT`  |

### Working with stacks

|                         | Git                                            | Sapling                      |
| ----------------------- | ---------------------------------------------- | ---------------------------- |
| Modify middle commit    | `git rebase -i`                                | `sl goto COMMIT && sl amend` |
| Move up/down the stack  | `git rebase -i`                                | `sl prev / sl next`          |
| Squash last two commits | `git reset --soft HEAD^ && git commit --amend` | `sl fold --from .^`          |
| Split a commit into two | `N/A`                                          | `sl split`                   |
| Reorder commits         | `git rebase -i`                                | `sl histedit`                |
| Amend down into stack   | `N/A`                                          | `sl absorb`                  |

### Giving commits names

|  | Git | Sapling |
| --- | --- | --- |
| Listing branches | `git branch` | `sl bookmark` |
| Create branch/bookmark | `git branch NAME` | `sl book NAME` |
| Switch to branch | `git checkout NAME` | `sl goto NAME` |
| Delete a branch | `git branch -d NAME` | `sl book -d NAME (deletes just the bookmark name)` / `sl book -D NAME` (deletes the bookmark name and hides the commits) |

### Resolving conflicts

|                           | Git                                    | Sapling              |
| ------------------------- | -------------------------------------- | -------------------- |
| List unresolved conflicts | `git diff --name-only --diff-filter=U` | `sl resolve --list`  |
| Mark a file resolved      | `git add FILE`                         | `sl resolve -m FILE` |
