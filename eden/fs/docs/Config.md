Eden Config Files
-----------------

The configuration parameters for Eden are stored in INI files. The default
'system' parameters are stored in the directory `/etc/eden/config.d/` and these
default parameters can be overridden by the user in the `~/.edenrc` user
configuration file.

When parsing configuration data, Eden loads everything in `/etc/eden/config.d/`
first, and then loads the data from `~/.edenrc` next. If the same section is
present in multiple files, the section found last wins, and entirely replaces
any contents of the section from previous files.

### Sample configuration file
***

```
[repository fbsource]
path = /data/users/$USER/fbsource
type = git

[bindmounts fbsource]
fbcode-buck-out = fbcode/buck-out
fbandroid-buck-out = fbandroid/buck-out
fbobjc-buck-out = fbobjc/buck-out
buck-out = buck-out
```

Each repository section includes the name of the repository in its header and
specifies the source of the repository in the 'path' field. The 'type' of the
repository must be either 'hg' or 'git.'

Each bindmounts section specifies the list of bindmounts corresponding to the
repository, where keys refer to the bind mount's directory name inside eden, and
values refer to the bind mount's mount path.

Please note that empty sections with only a header entry are not currently
supported.
