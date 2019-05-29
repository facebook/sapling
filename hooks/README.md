# Commit Hooks

This document explains how to author and configure commit hooks for Mononoke.

## Overview

Two types of hooks are supported (note that both are pre-commit hooks):

* `PerChangeset` runs once per changeset.
* `PerAddedOrModifiedFile` runs once per added or modified file per changeset.

Individual hooks are declared as follows in the `server.toml` file that
contains the configuration for a repo:

```toml
[[hooks]]
# The name of your hook. Must be unique within this file.
name="my_hook"

# Path to the Lua script that implements your hook.
# Relative paths are resolved relative to the root of the config repo.
path="my_hook.lua"

# This must be either "PerChangeset" or "PerAddedOrModifiedFile".
hook_type="PerAddedOrModifiedFile"

# The following property is optional.
# If specified and there is a line in the commit message that matches
# the specified value, then the hook is not run for that commit.
bypass_commit_string="@ignore-conflict-markers"

# The following property is optional.
# If specified, the hook can be bypassed by specifying `--pushvars KEY=VALUE`
# when running `hg push`.
bypass_pushvar="KEY=VALUE"
```

Enabled hooks must be declared in `[[bookmark.hooks]]`:

```toml
[[bookmarks.hooks]]
# Note how this matches the "name" property in [[hooks]].
hook_name="my_hook"
```

Here is an example of a hook to prevent Perl files from being checked in:

```lua
-- file my_hook.lua

function hook (ctx)
  if ctx.file.path:match("\\.perl$") then
    return false, "Boo: scary Perl!"
  else
    return true
end
```

## Lua API

Your hook must be implemented in Lua. The entry point to your hook must be a
global function named `hook()`. This function should return up to three
values:

* `success` (`boolean`) This should be `true` if the hook was satisfied and
  `false` if it was not. If this is `true`, then `description` and
  `long_description` must be `nil`.
* `description` (`string` or `nil`) If the hook was not satisfied, this
  must provide a short description of the failure, used to summarize this failure
  with other similar failures.
* `long_description` (`string` or `nil`) If the hook was not satisfied, this
  should provide a long description of the failure with suggestions for how the
  user should approach fixing this hook failure. Mononoke will use `description` if this
  is not provided, but this message needs to stand alone (`description` is not
  automatically added to this message).

Here are some common properties of hooks:

The `ctx` argument passed to the hook function always has at least the following
fields:

| key | description |
| --- | ----------- |
| `config_strings` | (`table of string`) Contains string configs defined per repository config |
| `config_ints` | (`table of int`) Contains int configs defined per repository config |
| `regex_match(regex, string)` | (`function`) Returns a `boolean` indicating whether the string matches the supplied regex |

The type `file` is a table with the following fields:

| key | description |
| --------- | ----------- |
| `path` | (`string`) Path to the file relative to the repo root. |
| `is_added()` | Returns a `boolean` indicating whether the file was added as part of the changeset |
| `is_deleted()` | Returns a `boolean` indicating whether the file was deleted as part of the changeset |
| `is_modified()` | Returns a `boolean` indicating whether the file was modified as part of the changeset
| `contains_string(needle)` | Returns a `boolean` indicating whether the specified string `needle` is present in this file. (Only present if `is_deleted()` returns `false`.) |
| `len()` | Returns a `number` that is the length of the file in bytes. (Only present if `is_deleted()` returns `false`.) |
| `content()` | Returns a `string` containing the contents of the file. (Only present if `is_deleted()` returns `false`.) |
| `path_regex_match(regex)` | Returns a `boolean` indicating whether the file's path matches the supplied regex |

### PerChangeset

Your `hook()` function receives a single `ctx` argument, which is a table with
the following additional fields:

| key | description |
| --------- | ----------- |
| `info` | (`table`) Described below. |
| `files` | (`table`) List of objects of type `file`, described above. |
| `file_content(path)` | (`function`) Takes the relative path to a file in the repo and returns its contents. |
| `parse_commit_msg()` | (`function`) Returns a table with phabricator tags parsed. |
| `is_valid_reviewer(user)` | (`function`) Returns whether a user can review the commit. |


`ctx.info` is a table with the following fields:

| key | description |
| --------- | ----------- |
| `author` | (`string`) The author of the changeset. This should be something like `"Stanislau Hlebik <stash@fb.com>"`. |
| `comments` | (`string`) The commit message. |
| `parent1_hash` | (`string` or `nil`) `p1` for the commit as a hex string, if it exists. |
| `parent2_hash` | (`string` or `nil`) `p2` for the commit as a hex string, if it exists. |

### PerAddedOrModifiedFile

Your `hook()` function receives a single `ctx` argument, which is a table with
the following additional fields:

| key | description |
| --------- | ----------- |
| `file` | (`table`) Object of type `file`, described above. (Note `is_deleted()` will return `false`.) |
