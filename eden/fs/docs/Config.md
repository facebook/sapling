## Eden Config Files

The configuration parameters for Eden are stored in INI files. The default
'system' parameters are stored in the directory `/etc/eden/` under `config.d`
and inside files `edenfs.rc` and `edenfs_dynamic.rc`. The files in `/etc/eden`
generally should not be edited by hand. Instead, these system configs can be
overridden by the user in the `~/.edenrc` user configuration file.

As of February 2024, the different system configs locations have the following
purposes:

- `/etc/eden/config.d/` is actively being removed to reduce confusion
- `/etc/eden/edenfs.rc` should contain "final"/"completed" config values,
  meaning the values can broadly be delivered to machines but for whatever
  reason shouldn't be hardcoded in the daemon
- `/etc/eden/edenfs_dynamic.rc` contains configs that gate active rollouts or
  configs that require a fine-grained delivery, e.g. on a per machine or per
  user basis

When parsing configuration data, Eden loads everything in `/etc/eden/config.d/`
first, `/etc/eden/edenfs.rc` second, and then `/etc/eden/edenfs_dynamic.rc`.
Finally, the data from `~/.edenrc` next.

If the same section is present in multiple files, the section found last wins,
and entirely replaces any contents of the section from previous files.

In the daemon, if any config is not present in the config files, the default
value from `EdenConfig.h` is used. If there is a config listed that EdenFS does
not know about, the daemon prints a log message at startup and ignores the
value.

In the CLI, if any config is not present in the config files, the default value
is defined inline at the config's usage point for that particular key in the
source code.

### Sample configuration setup

---

`/etc/eden/edenfs.rc`

```
[overlay]
buffered = true
inode-catalog-type = "lmdb"

[experimental]
enable-nfs-server = false
```

`/etc/eden/edenfs_dynamic.rc`

```
[experimental]
enable-nfs-server = true
```

`~/.edenrc`

```
[mount]
readdir-prefetch = "trees"
```

In the given example, EdenFS would get the config values from the following
locations

- `overlay:buffered = true # /etc/eden/edenfs.rc`
- `overlay:inode-catalog-type = "lmdb" # /etc/eden/edenfs.rc`
- `experimental:enable-nfs-server = true # /etc/eden/edenfs_dynamic.rc, overrides /etc/eden/edenfs.rc`
- `mount:readdir-prefetch = "trees" # ~/.edenrc, doesn't exist in any /etc/eden files`

Running `edenfsctl fsconfig` will provide an annotated view of EdenFS' configs,
similiar to the example above.

Please note that empty sections with only a header entry are not currently
supported.
