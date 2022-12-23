---
sidebar_position: 26
---

## pr
<!--
  @generated SignedSource<<11958e2f80f9a30ebe5618c3480062c3>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**exchange local commit data with GitHub pull requests**


## subcommands
### s|submit

create or update GitHub pull requests from local commits

| shortname | fullname | default | description |
| - | - | - | - |
| `-s`| `--stack`| `false`| also include draft ancestors|
| `-m`| `--message`| | message describing changes to updated commits|
| `-d`| `--draft`| `false`| mark new pull requests as draft|### pull

&#x69;mport a pull request into your working copy

The PULL_REQUEST can be specified as either a URL:

https://github.com/facebook/sapling/pull/321

or just the PR number within the GitHub repository identified by
`sl config paths.default`.

| shortname | fullname | default | description |
| - | - | - | - |
| `-g`| `--goto`| `false`| goto the pull request after importing it|### link

identify a commit as the head of a GitHub pull request

A PULL_REQUEST can be specified in a number of formats:

- GitHub URL to the PR: https://github.com/facebook/react/pull/42

- Integer: Number for the PR. Uses 'paths.upstream' as the target repo,    if specified; otherwise, falls back to 'paths.default'.

| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision to link|### unlink

remove a commit's association with a GitHub pull request

| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revisions to unlink|### follow

join the nearest desecendant's pull request

Marks commits to become part of their nearest desecendant's pull request
instead of starting as the head of a new pull request.

Use `pr unlink` to undo.

| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revisions to follow the next pull request|### list

calls `gh pr list [flags]` with the current repo as the value of --repo

| shortname | fullname | default | description |
| - | - | - | - |
| | `--app`| | filter by GitHub App author|
| `-a`| `--assignee`| | filter by assignee|
| `-A`| `--author`| | filter by author|
| `-B`| `--base`| | filter by base branch|
| `-d`| `--draft`| `false`| filter by draft state|
| `-H`| `--head`| | filter by head branch|
| `-q`| `--jq`| | filter JSON output using a jq expression|
| | `--json`| | output JSON with the specified fields|
| `-l`| `--label`| | filter by label|
| `-L`| `--limit`| `30`| maximum number of items to fetch (default 30)|
| `-S`| `--search`| | search pull requests with query|
| `-s`| `--state`| | filter by state: {open|closed|merged|all} (default "open")|
| `-t`| `--template`| | format JSON output using a Go template; see "gh help formatting"|
| `-w`| `--web`| `false`| list pull requests in the web browser|