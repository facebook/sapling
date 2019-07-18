# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init"

sh % "cat" << r"""
[extensions]
prefixfilter = prefix.py
[encode]
*.txt = stripprefix: Copyright 2046, The Masters
[decode]
*.txt = insertprefix: Copyright 2046, The Masters
""" > ".hg/hgrc"

sh % "cat" << r"""
from edenscm.mercurial import error
def stripprefix(s, cmd, filename, **kwargs):
    header = '%s\n' % cmd
    if s[:len(header)] != header:
        raise error.Abort('missing header "%s" in %s' % (cmd, filename))
    return s[len(header):]
def insertprefix(s, cmd):
    return '%s\n%s' % (cmd, s)
def reposetup(ui, repo):
    repo.adddatafilter('stripprefix:', stripprefix)
    repo.adddatafilter('insertprefix:', insertprefix)
""" > "prefix.py"

sh % "cat" << r"""
.gitignore
prefix.py
prefix.pyc
""" > ".gitignore"

sh % "cat" << r"""
Copyright 2046, The Masters
Some stuff to ponder very carefully.
""" > "stuff.txt"
sh % "hg add stuff.txt"
sh % "hg ci -m stuff"

# Repository data:

sh % "hg cat stuff.txt" == "Some stuff to ponder very carefully."

# Fresh checkout:

sh % "rm stuff.txt"
sh % "hg up -C" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "cat stuff.txt" == r"""
    Copyright 2046, The Masters
    Some stuff to ponder very carefully."""
sh % "echo 'Very very carefully.'" >> "stuff.txt"
sh % "hg stat" == "M stuff.txt"

sh % "echo 'Unauthorized material subject to destruction.'" > "morestuff.txt"

# Problem encoding:

sh % "hg add morestuff.txt"
sh % "hg ci -m morestuff" == r"""
    abort: missing header "Copyright 2046, The Masters" in morestuff.txt
    [255]"""
sh % "hg stat" == r"""
    M stuff.txt
    A morestuff.txt"""
