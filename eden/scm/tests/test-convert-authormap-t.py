# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % ". helpers-usechg.sh"

sh % "cat" << r"""
[extensions]
convert=
""" >> "$HGRCPATH"

# Prepare orig repo

sh % "hg init orig"
sh % "cd orig"
sh % "echo foo" > "foo"
sh % "'HGUSER=user name' hg ci -qAm foo"
sh % "cd .."

# Explicit --authors

sh % "cat" << r"""
user name = Long User Name

# comment
this line is ignored
""" > "authormap.txt"
sh % "hg convert --authors authormap.txt orig new" == r"""
    initializing destination new repository
    ignoring bad line in author map file authormap.txt: this line is ignored
    scanning source...
    sorting...
    converting...
    0 foo
    writing author map file $TESTTMP/new/.hg/authormap"""
sh % "cat new/.hg/authormap" == "user name=Long User Name"
sh % "hg -Rnew log" == r"""
    changeset:   0:d89716e88087
    tag:         tip
    user:        Long User Name
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     foo"""
sh % "rm -rf new"

# Implicit .hg/authormap

sh % "hg init new"
sh % "mv authormap.txt new/.hg/authormap"
sh % "hg convert orig new" == r"""
    ignoring bad line in author map file $TESTTMP/new/.hg/authormap: this line is ignored
    scanning source...
    sorting...
    converting...
    0 foo"""
sh % "hg -Rnew log" == r"""
    changeset:   0:d89716e88087
    tag:         tip
    user:        Long User Name
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     foo"""
