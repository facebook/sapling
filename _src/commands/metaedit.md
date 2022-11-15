---
sidebar_position: 23
---

## metaedit | meta | me
<!--
  @generated SignedSource<<fe1fb6ae018c5d011bf22851f6fe3543>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**edit commit message and other metadata**

`sl metaedit` lets you edit commit messages. With no
arguments, the current commit message is modified. To edit
the commit message for a different commit, specify `-r
REV`. To edit the commit messages for multiple commits,
specify `--batch`.

By default, `sl metaedit` launches your default editor so that
you can interactively edit the commit message. Specify `-m` to
specify the commit message on the command line.

You can edit other pieces of commit metadata such as the user or
date, by specifying `-u` or `-d`, respectively. The expected
format for the user is 'Full Name <user@example.com>'.

There is also an automation-friendly JSON input mode which allows
the caller to provide the mapping between commit and new message
and username in the following format:

```
{
    "<commit_hash>": {
        "message": "<message>",
        "user": "<user>" // optional
    }
}
```

You can specify `--fold` to fold multiple revisions into one when the
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
