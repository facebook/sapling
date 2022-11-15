#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

DEFAULT_ALIAS_DICT = {
    "restack": {
        "name": "restack",
        "aliases": ["restack"],
        "doc": """
rebase all commits in the current stack onto the latest version of their respective parents

`restack` is a built-in alias for `rebase --restack`

When commits are modified by commands like `amend` and `absorb`, their descendant
commits may be left behind as orphans. Rebase these orphaned commits onto the newest
versions of their ancestors, making the stack linear again.
""",
        "args": [],
        "subcommands": None,
    },
    "ssl": {
        "name": "ssl",
        "aliases": ["ssl"],
        "doc": """
show a graph of your commits and associated GitHub pull request status

`ssl` is not a subcommand, but a built-in alias for `smartlog -T {ssl}`.
If you have used Sapling to create pull requests for your commits, then
you can use `sl ssl` to include the pull request status in your Smartlog:

```
$ sl ssl
  @  4d9180fd8  6 minutes ago  alyssa  #178 Unreviewed
  │  adding baz
  │
  o  3cc43c835  6 minutes ago  alyssa  #177 Approved
  │  adding bar
  │
  o  4f1243a8b  6 minutes ago  alyssa  #176 Closed
╭─╯  adding foo
│
o  f22585511  Oct 06 at 17:40  remote/main
│
~
```
""",
        "args": [],
        "subcommands": None,
    },
}
