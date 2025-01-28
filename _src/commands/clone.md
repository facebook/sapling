---
sidebar_position: 9
---

## clone
<!--
  @generated SignedSource<<c87d240f56d0e2f7174b38dc26c20ce7>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**make a copy of an existing repository**

Create a copy of an existing repository in a new directory.

If no destination directory name is specified, it defaults to the
basename of the source.

The location of the source is added to the new repository&#x27;s
config file as the default to be used for future pulls.

Sources are typically URLs. The following URL schemes are assumed
to be a Git repo: `git`, `git+file`, `git+ftp`, `git+ftps`,
`git+http`, `git+https`, `git+ssh`, `ssh` and `https`.

Scp-like URLs of the form `user@host:path` are converted to
`ssh://user@host/path`.

Other URL schemes are assumed to point to an SaplingRemoteAPI capable repo.

The `--git` option forces the source to be interpreted as a Git repo.

To check out a particular version, use `-u/--update`, or
`-U/--noupdate` to create a clone with no working copy.

If specified, the `--enable-profile` option should refer to a
sparse profile within the source repo to filter the contents of
the new working copy. See `sl help -e sparse` for details.

Examples:

- clone a remote repository to a new directory named some_repo:

```
sl clone https://example.com/some_repo
```

As an experimental feature, if specified the source URL fragment
is persisted as the repo&#x27;s main bookmark.

Returns 0 on success.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-U`| `--noupdate`| `false`| clone an empty working directory|
| `-u`| `--updaterev`| | revision or branch to check out|
| | `--enable-profile`| | enable a sparse profile|
