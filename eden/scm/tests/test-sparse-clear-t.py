# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# test sparse

sh % "hg init myrepo"
sh % "cd myrepo"
sh % "cat" << r"""
[extensions]
sparse=
purge=
rebase=
""" >> "$HGRCPATH"

sh % "echo a" > "index.html"
sh % "echo x" > "data.py"
sh % "echo z" > "readme.txt"
sh % "cat" << r"""
[include]
*.sparse
""" > "base.sparse"
sh % "hg ci -Aqm initial"
sh % "cat" << r"""
%include base.sparse
[include]
*.html
""" > "webpage.sparse"
sh % "hg ci -Aqm initial"

# Clear rules when there are includes

sh % "hg sparse --include '*.py'"
sh % "ls" == "data.py"
sh % "hg sparse --clear-rules"
sh % "ls" == r"""
    base.sparse
    data.py
    index.html
    readme.txt
    webpage.sparse"""

# Clear rules when there are excludes

sh % "hg sparse --exclude '*.sparse'"
sh % "ls" == r"""
    data.py
    index.html
    readme.txt"""
sh % "hg sparse --clear-rules"
sh % "ls" == r"""
    base.sparse
    data.py
    index.html
    readme.txt
    webpage.sparse"""

# Clearing rules should not alter profiles

sh % "hg sparse --enable-profile webpage.sparse"
sh % "ls" == r"""
    base.sparse
    index.html
    webpage.sparse"""
sh % "hg sparse --include '*.py'"
sh % "ls" == r"""
    base.sparse
    data.py
    index.html
    webpage.sparse"""
sh % "hg sparse --clear-rules"
sh % "ls" == r"""
    base.sparse
    index.html
    webpage.sparse"""
