# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
sh % "hg init repo"
sh % "cd repo"
sh % "echo 0" > "a"
sh % "echo 0" > "b"
sh % "echo 0" > "t.h"
sh % "mkdir t"
sh % "echo 0" > "t/x"
sh % "echo 0" > "t/b"
sh % "echo 0" > "t/e.h"
sh % "mkdir dir.h"
sh % "echo 0" > "dir.h/foo"

sh % "hg ci -A -m m" == r"""
    adding a
    adding b
    adding dir.h/foo
    adding t.h
    adding t/b
    adding t/e.h
    adding t/x"""

sh % "touch nottracked"

sh % "hg locate a" == "a"

sh % "hg locate NONEXISTENT" == "[1]"

sh % "hg locate" == r"""
    a
    b
    dir.h/foo
    t.h
    t/b
    t/e.h
    t/x"""

sh % "hg rm a"
sh % "hg ci -m m"

sh % "hg locate a" == "[1]"
sh % "hg locate NONEXISTENT" == "[1]"
sh % "hg locate 'relpath:NONEXISTENT'" == "[1]"
sh % "hg locate" == r"""
    b
    dir.h/foo
    t.h
    t/b
    t/e.h
    t/x"""
sh % "hg locate -r 0 a" == "a"
sh % "hg locate -r 0 NONEXISTENT" == "[1]"
sh % "hg locate -r 0 'relpath:NONEXISTENT'" == "[1]"
sh % "hg locate -r 0" == r"""
    a
    b
    dir.h/foo
    t.h
    t/b
    t/e.h
    t/x"""

# -I/-X with relative path should work:

sh % "cd t"
sh % "hg locate" == r"""
    b
    dir.h/foo
    t.h
    t/b
    t/e.h
    t/x"""
sh % "hg locate -I ../t" == r"""
    t/b
    t/e.h
    t/x"""

# Issue294: hg remove --after dir fails when dir.* also exists

sh % "cd .."
sh % "rm -r t"

sh % "hg rm t/b"

sh % "hg locate 't/**'" == r"""
    t/b
    t/e.h
    t/x"""

sh % "hg files" == r"""
    b
    dir.h/foo
    t.h
    t/e.h
    t/x"""
sh % "hg files b" == "b"

sh % "mkdir otherdir"
sh % "cd otherdir"

sh % "hg files 'path:'" == r"""
    ../b
    ../dir.h/foo
    ../t.h
    ../t/e.h
    ../t/x"""
sh % "hg files 'path:.'" == r"""
    ../b
    ../dir.h/foo
    ../t.h
    ../t/e.h
    ../t/x"""

sh % "hg locate b" == r"""
    ../b
    ../t/b"""
sh % "hg locate '*.h'" == r"""
    ../t.h
    ../t/e.h"""
sh % "hg locate 'path:t/x'" == '../t/x'
sh % "hg locate 're:.*\\.h$'" == r"""
    ../t.h
    ../t/e.h"""
sh % "hg locate -r 0 b" == r"""
    ../b
    ../t/b"""
sh % "hg locate -r 0 '*.h'" == r"""
    ../t.h
    ../t/e.h"""
sh % "hg locate -r 0 'path:t/x'" == "../t/x"
sh % "hg locate -r 0 're:.*\\.h$'" == r"""
    ../t.h
    ../t/e.h"""

sh % "hg files" == r"""
    ../b
    ../dir.h/foo
    ../t.h
    ../t/e.h
    ../t/x"""
sh % "hg files ." == "[1]"

# Convert native path separator to slash (issue5572)

sh % "hg files -T '{path|slashpath}\\n'" == r"""
    ../b
    ../dir.h/foo
    ../t.h
    ../t/e.h
    ../t/x"""

sh % "cd ../.."
