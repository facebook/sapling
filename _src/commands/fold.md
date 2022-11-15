---
sidebar_position: 12
---

## fold | squash
<!--
  @generated SignedSource<<c3e296c77c7474661372f0d18a1ada98>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**combine multiple commits into a single commit**

With `--from`, fold all of the commit linearly between the current
commit and the specified commit.

With `--exact`, fold only the specified commits while ignoring the
current commit. The given commits must form a linear, continuous
chain.

Some examples:

- Fold from the current commit to its parent:

```
sl fold --from .^
```

- Fold all draft commits into the current commit:

```
sl fold --from 'draft()'
```

See `sl help phases` for more about draft commits and
`sl help revsets` for more about the `draft()` keyword.

- Fold commits between e254371c1 and be57079e4 into the current commit:

```
sl fold --from e254371c1::be57079e4
```

- Fold commits e254371c1 and be57079e4:

sl fold "e254371c1 + be57079e4" --exact

- Only fold commits linearly between foo and .:

```
sl fold foo::. --exact
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision to fold|
| | `--exact`| | only fold specified revisions|
| | `--from`| | fold linearly from current revision to specified revision|
| | `--no-rebase`| `false`| don't rebase descendants after fold|
| `-M`| `--reuse-message`| | reuse commit message from REV|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
