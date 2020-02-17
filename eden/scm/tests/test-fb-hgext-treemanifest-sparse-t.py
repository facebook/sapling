# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# test interaction between sparse and treemanifest (sparse file listing)

sh % "cat" << r"""
[extensions]
sparse=
treemanifest=
[treemanifest]
treeonly = True
[remotefilelog]
reponame = master
cachepath = $PWD/hgcache
""" >> "$HGRCPATH"

# Setup the repository

sh % "hg init myrepo"
sh % "cd myrepo"
sh % "touch show"
sh % "touch hide"
sh % "mkdir -p subdir/foo/spam subdir/bar/ham hiddensub/foo hiddensub/bar"
sh % "touch subdir/foo/spam/show"
sh % "touch subdir/bar/ham/hide"
sh % "touch hiddensub/foo/spam"
sh % "touch hiddensub/bar/ham"
sh % "hg add ." == r"""
    adding hiddensub/bar/ham
    adding hiddensub/foo/spam
    adding hide
    adding show
    adding subdir/bar/ham/hide
    adding subdir/foo/spam/show"""
sh % "hg commit -m Init"
sh % "hg sparse include show"
sh % "hg sparse exclude hide"
sh % "hg sparse include subdir"
sh % "hg sparse exclude subdir/foo"

# Test cwd

sh % "hg sparse cwd" == r"""
    - hiddensub
    - hide
      show
      subdir"""
sh % "cd subdir"
sh % "hg sparse cwd" == r"""
      bar
    - foo"""
sh % "hg sparse include foo"
sh % "hg sparse cwd" == r"""
      bar
      foo"""
