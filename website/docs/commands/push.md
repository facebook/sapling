---
sidebar_position: 29
---

## push
<!--
  @generated SignedSource<<03ca9179db39515dfd14b6e35e28aa55>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**push commits to the specified destination**

Push commits from the local repository to the specified
destination.

Use `-t/--to` to specify the remote bookmark. For Git repos,
remote bookmarks correspond to Git branches.

To add a named remote destination, see `sl path --add`.

`-r/--rev` specifies the commit(s) (including ancestors) to push to
the remote repository. Defaults to the current commit.

Add `--create` to create the remote bookmark if it doesn't already exist.

The `-f/--force` flag allows non-fast-forward pushes.

If DESTINATION is omitted, the default path will be used. See
`sl help urls` and `sl help path` for more information.

Examples:

- push your current commit to "main" on the default destination:

```
sl push --to main
```

- force push commit 05a82320d to "my-branch" on the "my-fork" destination:

```
sl push --rev 05a82320d my-fork --to my-branch --force
```

The `--pushvars` flag sends key-value metadata to the server.
For example, `--pushvars ENABLE_SOMETHING=true`. Push vars are
typically used to override commit hook behavior, or enable extra
debugging. Push vars are not supported for Git repos.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-f`| `--force`| | force push|
| `-r`| `--rev`| | a commit to push|
| | `--to`| | push revs to this bookmark|
| | `--non-forward-move`| | allows moving a remote bookmark to an arbitrary place|
| | `--create`| | create a new remote bookmark|
| `-d`| `--delete`| | delete remote bookmark|
