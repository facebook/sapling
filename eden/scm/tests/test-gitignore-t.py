# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "newrepo"
sh % "setconfig 'ui.gitignore=1' 'ui.hgignore=0'"

sh % "cat" << r"""
*.tmp
build/
""" > ".gitignore"

sh % "mkdir build exp"
sh % "cat" << r"""
!*
""" > "build/.gitignore"

sh % "cat" << r"""
!i.tmp
""" > "exp/.gitignore"

sh % "touch build/libfoo.so t.tmp Makefile exp/x.tmp exp/i.tmp"

sh % "hg status" == r"""
    ? .gitignore
    ? Makefile
    ? exp/.gitignore
    ? exp/i.tmp"""

# Test global ignore files

sh % "cat" << r"""
*.pyc
""" > "$TESTTMP/globalignore"

sh % "touch x.pyc"

sh % "hg status" == r"""
    ? .gitignore
    ? Makefile
    ? exp/.gitignore
    ? exp/i.tmp
    ? x.pyc"""

sh % "hg status --config 'ui.ignore.global=$TESTTMP/globalignore'" == r"""
    ? .gitignore
    ? Makefile
    ? exp/.gitignore
    ? exp/i.tmp"""
