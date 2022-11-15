---
sidebar_position: 27
---

## push
<!--
  @generated SignedSource<<fc28dfa645c084cfd048847014618a8e>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**push changes to the specified destination**

Push changesets from the local repository to the specified
destination.

This operation is symmetrical to pull: it is identical to a pull
in the destination repository from the current one.

By default, push will not allow creation of new heads at the
destination, since multiple heads would make it unclear which head
to use. In this situation, it is recommended to pull and merge
before pushing.

Extra care should be taken with the -f/--force option,
which will push all new heads on all branches, an action which will
almost always cause confusion for collaborators.

If -r/--rev is used, the specified revision and all its ancestors
will be pushed to the remote repository.

If -B/--bookmark is used, the specified bookmarked revision, its
ancestors, and the bookmark will be pushed to the remote
repository. Specifying `.` is equivalent to specifying the active
bookmark's name.

Please see `sl help urls` for important details about `ssh://`
URLs. If DESTINATION is omitted, a default path will be used.

The --pushvars option sends strings to the server that become
environment variables prepended with `HG_USERVAR_`. For example,
`--pushvars ENABLE_FEATURE=true`, provides the server side hooks with
`HG_USERVAR_ENABLE_FEATURE=true` as part of their environment.

pushvars can provide for user-overridable hooks as well as set debug
levels. One example is having a hook that blocks commits containing
conflict markers, but enables the user to override the hook if the file
is using conflict markers for testing purposes or the file format has
strings that look like conflict markers.

By default, servers will ignore `--pushvars`. To enable it add the
following to your configuration file:

```
[push]
pushvars.server = true
```

If `push.requirereason` is set to true, users will need to pass
`--pushvars PUSH_REASON="..."` in order to push, and their reason will
be logged via `ui.log(...)`.

`push.requirereasonmsg` can be used to set the message shown to users
when they don't provide a reason.

Returns 0 if push was successful, 1 if nothing to push.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-f`| `--force`| | force push|
| `-r`| `--rev`| | a changeset intended to be included in the destination|
| `-B`| `--bookmark`| | bookmark to push|
| `-t`| `--to`| | push revs to this bookmark|
| `-d`| `--delete`| | delete remote bookmark|
| | `--create`| | create a new remote bookmark|
| | `--allow-anon`| | allow a new unbookmarked head|
| | `--non-forward-move`| | allows moving a remote bookmark to an arbitrary place|
