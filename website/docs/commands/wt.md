---
sidebar_position: 3
---

## wt
<!--
  @generated <<SignedSource::*O*zOeWoEQle#+L!plEphiEmie@IsG>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**manage git working trees for parallel development**

Worktrees allow you to check out multiple commits simultaneously in
separate directories. This is useful for:

- Running multiple AI coding agents in parallel

- Reviewing PRs without disturbing your current work

- Testing changes in isolation

Subcommands:

add     Create a new working tree
list    List all working trees
remove  Remove a working tree

Use &#x27;sl wt SUBCOMMAND --help&#x27; for more information on a subcommand.


## subcommands
### add

create a new working tree for a commit

Creates a git worktree as a sibling directory and optionally copies
untracked config files using copy-on-write when possible.

NAME defaults to the short commit hash (e.g., &#x27;abc1234&#x27;).
COMMIT defaults to current working copy parent.

Worktrees are created as siblings (../&lt;repo&gt;-&lt;name&gt;) to avoid appearing
as untracked files in the main repo.

Configure files to copy:

```
sl config --local worktree.copyfiles ".env"
sl config --local worktree.copyfiles ".npmrc"
sl config --local worktree.copydirs "node_modules"
```

Examples:

```
# Create worktree for current commit (name = short hash)
sl wt add
```

```
# Create worktree with custom name
sl wt add feature-x
```

```
# Create worktree for specific commit
sl wt add review abc1234
```

```
# Skip copying untracked files
sl wt add --no-copy
```

| shortname | fullname | default | description |
| - | - | - | - |
| | `--no-copy`| `false`| skip copying untracked files|
### list

list all working trees

Shows all git worktrees associated with this repository.

### remove

remove a working tree

Removes a git worktree. Use --force to remove a worktree with
uncommitted changes.

Examples:

```
sl wt remove ../my-repo-feature-x
sl wt remove --force ../my-repo-feature-x
```

| shortname | fullname | default | description |
| - | - | - | - |
| `-f`| `--force`| `false`| force removal even with local changes|
