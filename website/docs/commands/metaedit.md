---
sidebar_position: 22
---

## metaedit | met | meta | metae | metaed | metaedi
<!--
  @generated SignedSource<<d258d9dd53079d80b6f6d188ba0ecdc0>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**edit commit message and other metadata**

Edit commit message for the current commit. By default, opens your default
editor so that you can edit the commit message interactively. Specify -m
to specify the commit message on the command line.

To edit the message for a different commit, specify -r. To edit the
messages of multiple commits, specify --batch.

You can edit other pieces of commit metadata, namely the user or date,
by specifying -u or -d, respectively. The expected format for user is
'Full Name <user@example.com>'.

There is also automation-friendly JSON input mode which allows the caller
to provide the mapping between commit and new message and username in the
following format:

```
{
    "<commit_hash>": {
        "message": "<message>",
        "user": "<user>" // optional
    }
}
```

You can specify --fold to fold multiple revisions into one when the
given revisions form a linear unbroken chain. However, `sl fold` is
the preferred command for this purpose. See `sl help fold` for more
information.

Some examples:

- Edit the commit message for the current commit:

```
sl metaedit
```

- Change the username for the current commit:

```
sl metaedit --user 'New User <new-email@example.com>'
```

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-r`| `--rev`| | revision to edit|
| | `--fold`| `false`| fold specified revisions into one|
| | `--batch`| `false`| edit messages of multiple commits in one editor invocation|
| | `--json-input-file`| | read commit messages and users from JSON file|
| `-M`| `--reuse-message`| | reuse commit message from another commit|
| `-m`| `--message`| | use text as commit message|
| `-l`| `--logfile`| | read commit message from file|
| `-d`| `--date`| | record the specified date as commit date|
| `-u`| `--user`| | record the specified user as committer|
