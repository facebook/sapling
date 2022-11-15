---
sidebar_position: 14
---

## ghstack
<!--
  @generated SignedSource<<454d5ddfde7a8cd37c159aa4e6e2c009>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**submits a stack of commits to GitHub as individual pull requests using ghstack**

Uses the scheme employed by ghstack (https://github.com/ezyang/ghstack) to
submit a stack of commits to GitHub as individual pull requests. Pull
requests managed by ghstack are never force-pushed.

Currently, you must configure ghstack by creating a ~/.ghstackrc file as
explained on https://github.com/ezyang/ghstack. Ultimately, we will likely
replace this use of the GitHub CLI to manage API requests to GitHub.

Note that you must have *write* access to the GitHub repository in order to
use ghstack. If you do not have write access, consider using the `pr`
subcommand instead.


## subcommands
### submit

submit stack of commits to GitHub

| shortname | fullname | default | description |
| - | - | - | - |
| `-m`| `--message`| `"Update"`| message describing changes to updated commits|
| `-u`| `--update-fields`| `false`| update GitHub pull request summary from the local commit|
| | `--short`| `false`| print only the URL of the latest opened PR to stdout|
| | `--force`| `false`| force push the branch even if your local branch is stale|
| | `--skip`| `false`| never skip pushing commits, even if the contents didn't change (use this if you've only updated the commit message).|
| | `--draft`| `false`| create the pull request in draft mode (only if it has not already been created)|### unlink

remove the association of a commit with a pull request

### land

lands the stack for the specified pull request URL

### checkout

goto the stack for the specified pull request URL

### action

goto the stack for the specified pull request URL

| shortname | fullname | default | description |
| - | - | - | - |
| | `--close`| `false`| close the specified pull request|