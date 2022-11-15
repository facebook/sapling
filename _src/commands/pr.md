---
sidebar_position: 25
---

## pr
<!--
  @generated SignedSource<<b9bfeddf53960c092b14a30544a4c24e>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**exchange local commit data with GitHub pull requests**


## subcommands
### submit

create or update GitHub pull requests from local commits

| shortname | fullname | default | description |
| - | - | - | - |
| `-s`| `--stack`| `false`| also include draft ancestors|
| `-m`| `--message`| | message describing changes to updated commits|### link

indentify a commit as the head of a GitHub pull request

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
| `-r`| `--rev`| | revisions to follow the next pull request|