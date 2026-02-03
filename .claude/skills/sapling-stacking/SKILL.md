---
name: Sapling Stacking Workflow
description: Managing stacked PRs with Sapling's branchless workflow, automatic restacking, worktrees, and GitHub integration
when-to-use: |
  - Creating commits and stacked PRs
  - Navigating and managing commit stacks
  - Submitting PRs for review
  - Amending commits in a stack
  - Rebasing stacks and resolving merge conflicts
  - Working in parallel with worktrees
  - Reviewing PRs without disturbing current work
---

# Sapling Stacking Workflow

## Overview

Sapling (`sl`) is a source control system designed for stacked diffs workflow. Its key differentiator is **branchless design** - you work directly with commits, and branches are optional. Sapling automatically restacks dependent commits when you amend, making stack management effortless.

## When to Use This Skill

Use Sapling commands when:

- Creating commits (no branch needed)
- Managing stacked PRs
- Navigating your commit stack
- Amending commits and updating PRs
- Absorbing fixes across multiple commits
- Rebasing stacks onto main or other branches
- Resolving merge conflicts during rebase

## Core Concepts

### Branchless Workflow

Unlike Git, Sapling doesn't require branches:

```bash
# Git requires branches
git checkout -b feature/my-feature
git commit -m "my change"

# Sapling - just commit directly
sl commit -m "my change"
```

Your commits form a stack automatically. Navigate with `sl prev`/`sl next` or `sl goto`.

### Smartlog

View your commit graph with the smartlog:

```bash
sl        # Basic smartlog
sl ssl    # Smartlog with PR status
```

**Smartlog symbols:**

| Symbol | Meaning                            |
| ------ | ---------------------------------- |
| `@`    | Current commit (where you are)     |
| `○`    | Other commits in your stack        |
| `◉`    | Public/remote commits (main, etc.) |

## Creating Commits

### First Commit (New Stack)

```bash
# Add new files
sl add libs/vendure/user-plugin/src/entity/user.entity.ts

# Create commit
sl commit -m "add user entity"
```

### Subsequent Commits (Build the Stack)

```bash
# Tracked files are committed automatically
sl commit -m "add user service"

# Or use interactive mode to select changes
sl commit -i -m "add user resolver"
```

**Key difference from Git:** No staging area for tracked files. Use `sl add` only for new files.

## Navigation Commands

```bash
sl prev           # Move to previous commit (down the stack)
sl next           # Move to next commit (up the stack)
sl goto <hash>    # Jump to specific commit
sl goto top       # Go to top of current stack
sl goto bottom    # Go to bottom of current stack
```

## Pull Request Workflow

### Creating PRs

**Submit entire stack as separate PRs:**

```bash
sl pr submit --stack
# or shorthand:
sl pr s -s
```

**Submit only the current commit:**

```bash
sl pr submit
```

**As draft:**

```bash
sl pr submit --draft
sl pr submit --stack --draft
```

### Viewing PR Status

```bash
sl ssl          # Smartlog with PR numbers and status
sl pr list      # List all your PRs
```

### Updating PRs After Changes

After amending commits in your stack:

```bash
# Make your changes
sl goto <commit>
sl amend

# Push updated stack to GitHub
sl pr submit --stack
```

Sapling automatically restacks dependent commits after amend.

## Amending Commits

### Amend Current Commit

```bash
# Make changes to files
vim entity.ts

# Amend the current commit
sl amend

# Sapling automatically restacks dependent commits!
```

### Amend a Specific Commit

```bash
# Go to the commit you want to amend
sl goto <commit-hash>

# Make changes
vim file.ts

# Amend
sl amend

# Return to top of stack
sl goto top
```

## Absorbing Changes (Smart Fixups)

`sl absorb` is one of Sapling's killer features - it automatically distributes staged changes to the correct commits in your stack by analyzing which commit last modified each line.

### Basic Usage

```bash
# Make fixes across multiple files
sl add entity.ts    # Fix belongs to commit A
sl add service.ts   # Fix belongs to commit B

# Absorb figures out which commit each change belongs to
sl absorb

# Then push updated stack
sl pr submit --stack
```

### When Absorb Shines

**Addressing code review feedback:**

```bash
# Reviewer left comments on multiple commits in your stack
# Make all the fixes in your working directory

sl add <all-fixed-files>
sl absorb              # Each fix goes to the right commit
sl pr submit --stack   # Update all PRs at once
```

**Fixing typos across a stack:**

```bash
# Found typos in commits A, B, and C
# Fix them all, then absorb

sl absorb
```

### When Absorb Can't Help

Absorb only works when the lines you're changing were **previously modified** by commits in your stack. For new code or lines untouched by your stack:

```bash
# Amend to a specific commit
sl amend --to <commit-hash>

# Or navigate and amend manually
sl goto <commit-hash>
sl amend
sl goto top
```

## Stack Management

### Splitting Commits

```bash
sl split
# Interactive editor opens
# Select hunks for first commit, provide message
# Remaining changes become second commit
```

### Folding Commits

Combine multiple commits into one:

```bash
# Fold from a commit to current
sl fold --from <commit-hash>

# Fold specific adjacent commits
sl fold --exact <commit1> <commit2>
```

### Hiding Commits

Instead of deleting, hide commits (recoverable):

```bash
sl hide <hash>      # Hide commit
sl unhide <hash>    # Restore commit
sl log --hidden     # View hidden commits
```

## Syncing with Remote

```bash
sl pull              # Fetch changes from remote
sl rebase -d main    # Rebase stack onto main
sl push              # Push changes
```

### After PR is Merged

```bash
sl pull              # Fetch merged changes
sl rebase -d main    # Rebase remaining stack onto main
sl pr submit --stack # Update remaining PRs
```

## Worktrees (Parallel Development)

Worktrees let you check out multiple commits simultaneously in separate directories. This is useful for:

- Running multiple AI coding agents in parallel
- Reviewing PRs without disturbing your current work
- Testing changes in isolation

### Creating Worktrees

```bash
# Create worktree for current commit (named repo-<hash>)
sl wt add
# Creates ../marketplace-abc1234/

# Create worktree with custom name
sl wt add feature-review
# Creates ../marketplace-feature-review/

# Create worktree for specific commit
sl wt add pr-123 def5678
# Creates ../marketplace-pr-123/ at commit def5678
```

### Managing Worktrees

```bash
# List all worktrees
sl wt list

# Remove a worktree
sl wt remove ../marketplace-feature-review

# Force remove (if uncommitted changes)
sl wt remove --force ../marketplace-feature-review
```

### Pull PR Stack into Worktree

Review a PR without affecting your current work:

```bash
# Import PR stack into a new worktree
sl pr get --wt 123
# Creates ../marketplace-pr-123/ with full PR stack

# Custom worktree name
sl pr get --wt --wt-name review-alice 456
# Creates ../marketplace-review-alice/
```

### Automatic File Copying

When creating worktrees, these untracked files are automatically copied (using copy-on-write for efficiency):

**Files:** `.env`, `.envrc`, `.env.local`, `.tool-versions`, `mise.toml`, `.claude/settings.local.json`

**Directories:** `node_modules`

Configure custom files:

```bash
sl config --local worktree.copyfiles ".npmrc"
sl config --local worktree.copydirs "vendor"
```

Skip copying:

```bash
sl wt add --no-copy
```

### Worktree Workflow Example

```bash
# You're working on feature-x
sl status  # Some uncommitted changes

# Need to review PR #42 urgently
sl pr get --wt 42
cd ../marketplace-pr-42

# Review, test, comment on the PR
yarn test
# ...

# Done reviewing, back to your work
cd ../marketplace
# Your uncommitted changes are still there
```

## Rebasing and Conflict Resolution

### Rebase Commands

```bash
# Rebase current stack onto main
sl rebase -d main

# Rebase onto a specific commit
sl rebase -d <commit-hash>

# Rebase onto remote main (fetch first)
sl pull && sl rebase -d main
```

### Handling Merge Conflicts

When a rebase encounters conflicts, Sapling pauses and marks the conflicting files. The key to good conflict resolution is **understanding the intent** of the target branch's changes before resolving.

**Step 1: Identify conflicting files**

```bash
sl status    # Shows files with conflicts (marked with U for unresolved)
```

**Step 2: Examine target branch changes BEFORE resolving**

This is the critical step most people skip. Before touching the conflict markers, understand what happened in the target branch:

```bash
# View recent changes to the conflicting file in the target branch
sl log -p -l 3 -r "ancestors(main)" <conflicting-file>

# Or use git command (works in Sapling's git mode)
git log -p -n 3 main -- <conflicting-file>
```

This reveals:

- What refactoring was done
- Why functions were moved or renamed
- The intent behind structural changes

**Step 3: Resolve with understanding**

Now that you understand both sides:

- Preserve the target branch's refactoring/structure
- Adapt your changes to work within the new structure
- Don't blindly accept "ours" or "theirs"

**Step 4: Mark resolved and continue**

```bash
# After resolving conflicts in a file
sl resolve -m <file>

# Or mark all as resolved
sl resolve -m -a

# Continue the rebase
sl rebase --continue
```

### Conflict Resolution Strategy

| Situation                     | Approach                                |
| ----------------------------- | --------------------------------------- |
| Target refactored a file      | Adapt your changes to new structure     |
| Target renamed a function     | Update your code to use new name        |
| Target moved code to new file | Move your changes to the new location   |
| Both modified same logic      | Understand both intents, merge manually |

### Aborting a Rebase

If the conflicts are too complex or you need to rethink your approach:

```bash
sl rebase --abort    # Return to pre-rebase state
```

### Common Rebase Scenarios

**Scenario: Main has been updated**

```bash
sl pull
sl rebase -d main
# Resolve any conflicts
sl rebase --continue
sl pr submit --stack
```

**Scenario: Bottom PR merged, rebase remaining stack**

```bash
sl pull
sl rebase -d main
sl pr submit --stack
```

**Scenario: Need to rebase onto a different branch**

```bash
sl rebase -d <branch-name>
```

### Rebase vs Merge

Sapling is designed for rebasing, not merging. Always rebase your stacks:

- ✅ `sl rebase -d main` - Clean linear history
- ❌ `sl merge main` - Creates merge commits, breaks stack workflow

## Quick Reference

| Task                   | Command                  |
| ---------------------- | ------------------------ |
| View commit graph      | `sl`                     |
| View with PR status    | `sl ssl`                 |
| Create commit          | `sl commit -m "message"` |
| Amend current commit   | `sl amend`               |
| Navigate down          | `sl prev`                |
| Navigate up            | `sl next`                |
| Jump to commit         | `sl goto <hash>`         |
| Submit PR              | `sl pr submit`           |
| Submit stack           | `sl pr submit --stack`   |
| Pull changes           | `sl pull`                |
| Rebase onto main       | `sl rebase -d main`      |
| Continue rebase        | `sl rebase --continue`   |
| Abort rebase           | `sl rebase --abort`      |
| Mark conflict resolved | `sl resolve -m <file>`   |
| Absorb fixes           | `sl absorb`              |
| Split commit           | `sl split`               |
| Hide commit            | `sl hide <hash>`         |
| Launch web UI          | `sl web`                 |
| Create worktree        | `sl wt add [name]`       |
| List worktrees         | `sl wt list`             |
| Remove worktree        | `sl wt remove <path>`    |
| PR to worktree         | `sl pr get --wt <num>`   |

## Command Comparison: Sapling vs Git

| Task             | Sapling               | Git                          |
| ---------------- | --------------------- | ---------------------------- |
| View history     | `sl`                  | `git log --graph`            |
| Commit           | `sl commit -m`        | `git add . && git commit -m` |
| Amend            | `sl amend`            | `git commit --amend`         |
| Move in stack    | `sl prev` / `sl next` | `git checkout <hash>`        |
| Submit PR        | `sl pr submit`        | `gh pr create`               |
| Rebase           | Automatic on amend    | `git rebase -i`              |
| Interactive edit | `sl split`            | `git rebase -i`              |
| Smart fixup      | `sl absorb`           | `git absorb` (separate tool) |

## Integration with Other Workflows

### With Database Migrations

```bash
# After entity changes
yarn migration:generate addUserEmail
yarn migration:compile

# Commit entity + migrations together
sl add libs/vendure/user-plugin/src/entity/ migrations/
sl commit -m "add email field to User entity"
```

### With GraphQL Type Generation

```bash
# After schema changes
yarn generate-types

# Commit schema + generated types together
sl add libs/vendure/user-plugin/src/api/
sl add libs/graphql/admin/src/lib/
sl commit -m "add user email field to GraphQL schema"
```

### With Lint/Build Fixes

```bash
# Fix errors
yarn lint && yarn build

# Amend fixes into current commit
sl amend

# Or create new commit
sl commit -m "fix lint and build errors"
```

## Pre-Commit Checklist

Before using Sapling commands:

- [ ] Creating new commit? → `sl add <new-files>` then `sl commit -m "msg"`
- [ ] Adding to existing commit? → Make changes then `sl amend`
- [ ] Fixing multiple commits? → `sl add <fixes>` then `sl absorb`
- [ ] Ready to submit? → `sl pr submit --stack`
- [ ] Starting session? → `sl pull` then `sl rebase -d main`

## Web UI

Sapling includes a built-in interactive web UI:

```bash
sl web
```

This launches an interface where you can:

- Visualize your commit graph
- Drag and drop commits to rebase
- Navigate the repository visually

## ReviewStack Integration

PRs are viewed on ReviewStack for better visualization of stacked commits:

```
https://reviews.qlax.dev/{owner}/{repo}/pull/{number}
```

Configure as default:

```bash
sl config --user github.pull_request_domain reviews.qlax.dev
```

## Caveats When Mixing sl and git Commands

- **Interrupted operations**: Use matching tool to continue (`sl rebase --continue` for sl, `git rebase --continue` for git)
- **File tracking**: Use `sl add` for new files, `git add` may not affect Sapling's tracking
- **GitHub-only**: `sl pr` only supports GitHub (not GitLab, Bitbucket)

---

**Remember:** Sapling is branchless by design. Just commit, amend, and let Sapling handle the restacking automatically.
