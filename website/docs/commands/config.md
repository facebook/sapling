---
sidebar_position: 10
---

## config | conf
<!--
  @generated SignedSource<<f2bd182cd96f4902a67381f7484ac865>>
  Run `./scripts/generate-command-markdown.py` to regenerate.
-->


**show config settings**

With no arguments, print names and values of all config items.

With one argument of the form `section.name`, print just the value
of that config item.

With multiple arguments, print names and values of all config
items with matching section names.

With `--user`, edit the user-level config file. With `--system`,
edit the system-wide config file. With `--local`, edit the
repository-level config file. If there are no arguments, spawn
an editor to edit the config file. If there are arguments in
`section.name=value` or `section.name value` format, the appropriate
config file will be updated directly without spawning an editor.

With `--delete`, the specified config items are deleted from the config
file.

With `--debug`, the source (filename and line number) is printed
for each config item.

See `sl help config` for more information about config files.

Returns 0 on success, 1 if NAME does not exist.

## arguments
| shortname | fullname | default | description |
| - | - | - | - |
| `-u`| `--user`| `false`| edit user config, opening in editor if no args given|
| `-l`| `--local`| `false`| edit repository config, opening in editor if no args given|
| `-s`| `--system`| `false`| edit system config, opening in editor if no args given|
| `-d`| `--delete`| `false`| delete specified config items|
