---
sidebar_position: 100
---

import {Command} from '@site/elements'

# Shelve

The Sapling <Command name="shelve" /> command allows you to temporarily put pending changes off to the side, then bring them back later. Any pending changes in the working copy will be saved, reverting the working copy back to a clean state. Shelves can be named with `-n` for easier identification.

It is similar to the `git stash` command.

```bash
$ vim myproject.cpp
$ sl status
M myproject.cpp

$ sl shelve
$ sl status
```

You can either use `sl unshelve` to restore the latest shelved change to the working copy, `sl unshelve [shelved name]` to specify a change to unshelve.

```bash
$ sl status

$ sl unshelve
$ sl status
M myproject.spp
```
