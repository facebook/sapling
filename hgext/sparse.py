# sparse.py - shim that redirects to load fbsparse
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""allow sparse checkouts of the working directory

Sparse file format
------------------

Structure
.........

Shared sparse profile files comprise of 4 sections: `%include` directives
that pull in another sparse profile, and `[metadata]`, `[include]` and
`[exclude]` sections.

Any line starting with a `;` or `#` character is a comment and is ignored.

Extending existing profiles
...........................

`%include <absolute path>` directives (one per line) let you extend as
an existing profile file, adding more include and exclude rules. Although
this directive can appear anywere in the file, it is recommended you
keep these at the top of the file.

Metadata
........

The `[metadata]` section lets you specify key-value pairs for the profile.
Anything before the first `:` or `=` is the key, everything after is the
value. Values can be extended over multiple lines by indenting additional
lines.

Only the `title`, `description` and `hidden` keys carry meaning to for
`hg sparse`, these are used in the `hg sparse list` and
`hg sparse explain` commands. Profiles with the `hidden` key (regardless
of its value) are excluded from the `hg sparse list` listing unless
the `-v` / `--verbose` switch is given.

Include and exclude rules
.........................

Each line in the `[include]` and `[exclude]` sections is treated as a
standard pattern, see :hg:`help patterns`. Exclude rules override include
rules.

Example
.......

::

  # this profile extends another profile, incorporating all its rules
  %include some/base/profile

  [metadata]
  title: This is an example sparse profile
  description: You can include as much metadata as makes sense for your
    setup, and values can extend over multiple lines.
  lorem ipsum = Keys and values are separated by a : or =
  ; hidden: the hidden key lets you mark profiles that should not
  ;  generally be discorable. The value doesn't matter, use it to motivate
  ;  why it is hidden.

  [include]
  foo/bar/baz
  bar/python_project/**/*.py

  [exclude]
  ; exclude rules override include rules, so all files with the extension
  ; .ignore are excluded from this sparse profile.
  foo/bar/baz/*.ignore

Configuration options
---------------------

The following config option defines whether sparse treats supplied
paths as relative to repo root or to the current working dir for
include and exclude options:

    [sparse]
    includereporootpaths = off

The following config option defines whether sparse treats supplied
paths as relative to repo root or to the current working dir for
enableprofile and disableprofile options:

    [sparse]
    enablereporootpaths = on

You can configure a path to find sparse profiles in; this path is
used to discover available sparse profiles. Nested directories are
reflected in the UI.

    [sparse]
    profile_directory = tools/scm/sparse

It is not set by default.
"""
from __future__ import absolute_import

from . import fbsparse

cmdtable = fbsparse.cmdtable.copy()

def _fbsparseexists(ui):
    with ui.configoverride({("devel", "all-warnings"): False}):
        return not ui.config('extensions', 'fbsparse', '!').startswith('!')

def uisetup(ui):
    if _fbsparseexists(ui):
        cmdtable.clear()
        return
    fbsparse.uisetup(ui)

def extsetup(ui):
    if _fbsparseexists(ui):
        cmdtable.clear()
        return
    fbsparse.extsetup(ui)

def reposetup(ui, repo):
    if _fbsparseexists(ui):
        return
    fbsparse.reposetup(ui, repo)
