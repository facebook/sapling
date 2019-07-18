# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test various flags to turn off bad hg features.

sh % "newrepo"
sh % "drawdag" << r"""
A
"""
sh % "hg up -Cq '$A'"

# Test disabling the `hg merge` command:
sh % "hg merge" == r"""
    abort: nothing to merge
    [255]"""
sh % "setconfig 'ui.allowmerge=False'"
sh % "hg merge" == r"""
    abort: merging is not supported for this repository
    (use rebase instead)
    [255]"""

# Test disabling the `hg tag` command:
sh % "hg tag foo"
sh % "hg tags" == r"""
    tip                                1:9b0f5d3c138d
    foo                                0:426bada5c675"""
sh % "setconfig 'ui.allowtags=False'"
sh % "hg tag foo2" == r"""
    abort: new tags are disabled in this repository
    [255]"""
sh % "hg tags" == r"""
    abort: tags are disabled in this repository
    [255]"""

# Test disabling the `hg branch` commands:
sh % "hg branch" == r"""
    default
    hint[branch-command-deprecate]: 'hg branch' command does not do what you want, and is being removed. It always prints 'default' for now. Check fburl.com/why-no-named-branches for details.
    hint[hint-ack]: use 'hg hint --ack branch-command-deprecate' to silence these hints"""
sh % "setconfig 'ui.allowbranches=False'"
sh % "hg branch foo" == r"""
    abort: named branches are disabled in this repository
    (use bookmarks instead)
    [255]"""
sh % "setconfig 'ui.disallowedbrancheshint=use bookmarks instead! see docs'"
sh % "hg branch -C" == r"""
    abort: named branches are disabled in this repository
    (use bookmarks instead! see docs)
    [255]"""
