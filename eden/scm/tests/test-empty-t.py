# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401

sh % "configure modernclient"

# Create an empty repo:

sh % "newclientrepo a"

# Try some commands:

sh % "hg log"
sh % "hg histgrep wah" == "[1]"
sh % "hg manifest"

# Poke at a clone:
sh % "hg push -r . -q --to book --create"

sh % "cd .."
sh % "newclientrepo b test:a_server" == ""
sh % "hg log" == ""
