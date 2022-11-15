---
sidebar_position: 30
---
# Git cheat sheet

Below is a quick cheat sheet for translating a number of Git commands into equivalent Sapling commands.

You can also use the `sl githelp` command, or `sl git` for short, to automatically translate some git commands into their equivalent Sapling command.

```
$ sl githelp -- git clone https://github.com/facebook/sapling
sl clone https://github.com/facebook/sapling

$ sl git -- checkout 060f340a9 my_file.txt
sl revert -r 060f340a9 my_file.txt
```

### Cloning and pulling a repository

| |Git |Sapling |
|--- |--- |--- |
|Clone  |git clone http://github.com/foo my_repo |sl clone http://github.com/foo my_repo |
|Pull |git fetch |sl pull |
|Pull a Branch |git fetch REFSPEC |sl pull -B my_branch |
|Pull From |git fetch origin |sl pull remote |
|Pull and Rebase |git pull |sl pull —rebase |

### Understanding the repository

| |Git |Sapling |
|--- |--- |--- |
|Your Commits |N/A |sl |
|Current History |git log |sl log |
|Edited Files |git status |sl status |
|Current Hash |git rev-parse HEAD |sl whereami |
|Pending Changes |git diff |sl diff |
|Current Commit |git show |sl show |

### Referring to commits

| |Git |Sapling |
|--- |--- |--- |
|Current Commit |HEAD |. |
|Parent Commit |HEAD^ |.^ |
|All local commits |N/A |draft() |
|Commits in branch X but not Y |Y..X |X % Y |

See `sl help revset` for more ways of referencing commits.

### Working with files

| |Git |Sapling |
|--- |--- |--- |
|Add New File |git add FILE |sl add FILE |
|Un-add New File |git rm --cached FILE |sl forget FILE |
|Remove File |git rm FILE |sl rm FILE |
|Rename File |git mv OLD NEW |sl mv OLD NEW |
|Copy File |cp OLD NEW |sl cp OLD NEW |
|Add/Remove All Files |git add -A . |sl addremove |
|Undo Changes |git checkout -- FILE |sl revert FILE |
|Undo All Changes |git reset --hard |sl revert —all |
|Delete Untracked Files |git clean -f |sl clean |
|Output File Content |git cat-file -p COMMIT:FILE |sl cat -r COMMIT FILE |
|Show Blame |git blame FILE |sl blame FILE |

### Working with commits

| |Git |Sapling |
|--- |--- |--- |
|Commit Changes |git commit -a |sl commit |
|Modify Commit |git commit -a --amend |sl amend |
|Move to Commit |git checkout COMMIT |sl goto COMMIT |
|Remove a Commit |git reset -hard COMMIT |sl hide COMMIT |
|Edit Message |git commit --amend |sl metaedit |
|Rebase Commits |git rebase main |sl rebase -d main |
|Complex Rebase |git rebase --onto DEST BOTTOM^ TOP |sl rebase -d DEST -r BOTTOM::TOP |
|Rebase All |N/A |sl rebase -d main -r 'draft()' |
|Interactive Rebase |git rebase -i |sl histedit |
|Interactive Commit |git add -p |sl commit -i / sl amend -i  |
|Cherry-pick |git cherry-pick COMMIT |sl graft COMMIT |
|Stash Changes |git stash |sl shelve |
|Unstach Changes |git stash pop |sl unshelve |

### Undo, redo, and reverting

| |Git |Sapling |
|--- |--- |--- |
|Undo Commit |git reset --soft HEAD^ |sl uncommit |
|Undo Partial Commit |git reset --soft HEAD^ FILE |sl uncommit FILE |
|Undo Amend |git reset HEAD@{1} |sl unamend |
|Undo Rebase/Etc |git reset —hard HEAD@{1} |sl undo |
|Revert Already Landed Commit |git revert COMMIT |sl backout COMMIT |
|View Recent Commits |git reflog |sl journal |
|Recover Commit |git reset COMMIT |sl unhide COMMIT |

### Working with stacks

| |Git |Sapling |
|--- |--- |--- |
|Modify Middle Commit |git rebase -i |sl goto COMMIT && sl amend |
|Move Up/Down the Stack |git rebase -i |sl prev / sl next |
|Squash Last Two Commits |git reset --soft HEAD^ && git commit —amend |sl fold —from .^ |
|Split a Commit into Two |N/A |sl split |
|Reorder Commits |git rebase -i |sl histedit |
|Amend Down into Stack |N/A |sl absorb |

### Giving commits names

| |Git |Sapling |
|--- |--- |--- |
|Listing Branches |git branch |sl bookmark |
|Create Branch/Bookmark |git branch NAME |sl book NAME |
|Switch to Branch |git checkout NAME |sl goto NAME |
|Delete Branch |git branch -d NAME |sl book -d NAME (deletes just the bookmark name) / sl book -D NAME (deletes the bookmark name and hides the commits) |

### Resolving conflicts

| |Git |Sapling |
|--- |--- |--- |
|List Unresolved Conflicts |git diff —name-only —diff-filter=U |sl resolve —list |
|Mark a File Resolved |git add FILE |sl resolve -m FILE |
