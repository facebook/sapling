# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# test sparse interaction with other extensions

sh % "hg init myrepo"
sh % "cd myrepo"
sh % "cat" << r"""
[extensions]
sparse=
# Remove once default-on:
simplecache=
[simplecache]
showdebug=true
cachedir=$TESTTMP/hgsimplecache
""" > ".hg/hgrc"

# Test integration with simplecache for profile reads

sh % "printf '[include]\\nfoo\\n.gitignore\\n'" > ".hgsparse"
sh % "hg add .hgsparse"
sh % "hg commit -qm 'Add profile'"
sh % "hg sparse --enable-profile .hgsparse"
sh % "hg status --debug" == "got value for key sparseprofile:.hgsparse:090ca0df22bcfedb0d8c8cb8c66865529e714404:v2 from local"

if feature.check(["fsmonitor"]):
    # Test fsmonitor integration (if available)

    sh % "touch .watchmanconfig"
    sh % "echo ignoredir1" >> ".gitignore"
    sh % "hg commit -Am ignoredir1" == "adding .gitignore"
    sh % "echo ignoredir2" >> ".gitignore"
    sh % "hg commit -m ignoredir2"

    sh % "hg sparse reset"
    sh % "hg sparse -I ignoredir1 -I ignoredir2 -I dir1 -I .gitignore"

    sh % "mkdir ignoredir1 ignoredir2 dir1"
    sh % "touch ignoredir1/file ignoredir2/file dir1/file"

    # Run status twice to compensate for a condition in fsmonitor where it will check
    # ignored files the second time it runs, regardless of previous state (ask @sid0)
    sh % "hg status" == "? dir1/file"
    sh % "hg status" == "? dir1/file"

    # Test that fsmonitor by default handles .gitignore changes and can "unignore" files.

    sh % "hg up -q '.^'"
    sh % "hg status" == r"""
        ? dir1/file
        ? ignoredir2/file"""
