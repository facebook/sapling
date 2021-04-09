# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "mkdir folder"
sh % "cd folder"
sh % "hg init"
sh % "mkdir x x/l x/m x/n x/l/u x/l/u/a"
sh % "touch a b x/aa.o x/bb.o"
sh % "hg status" == r"""
    ? a
    ? b
    ? x/aa.o
    ? x/bb.o"""

sh % "hg status --terse u" == r"""
    ? a
    ? b
    ? x/"""
sh % "hg status --terse maudric" == r"""
    ? a
    ? b
    ? x/"""
sh % "hg status --terse madric" == r"""
    ? a
    ? b
    ? x/aa.o
    ? x/bb.o"""
sh % "hg status --terse f" == r"""
    abort: 'f' not recognized
    [255]"""

# Add a .gitignore so that we can also have ignored files

sh % "echo '*\\.o'" > ".gitignore"
sh % "hg status" == r"""
    ? .gitignore
    ? a
    ? b"""
sh % "hg status -i" == r"""
    I x/aa.o
    I x/bb.o"""

# Tersing ignored files
sh % "hg status -t i --ignored" == "I x/"

# Adding more files
sh % "mkdir y"
sh % "touch x/aa x/bb y/l y/m y/l.o y/m.o"
sh % "touch x/l/aa x/m/aa x/n/aa x/l/u/bb x/l/u/a/bb"

sh % "hg status" == r"""
    ? .gitignore
    ? a
    ? b
    ? x/aa
    ? x/bb
    ? x/l/aa
    ? x/l/u/a/bb
    ? x/l/u/bb
    ? x/m/aa
    ? x/n/aa
    ? y/l
    ? y/m"""

sh % "hg status --terse u" == r"""
    ? .gitignore
    ? a
    ? b
    ? x/
    ? y/"""

sh % "hg add x/aa x/bb .gitignore"
sh % "hg status --terse au" == r"""
    A .gitignore
    A x/aa
    A x/bb
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/"""

# Including ignored files

sh % "hg status --terse aui" == r"""
    A .gitignore
    A x/aa
    A x/bb
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/l
    ? y/m"""
sh % "hg status --terse au -i" == r"""
    I x/aa.o
    I x/bb.o
    I y/l.o
    I y/m.o"""

# Committing some of the files

sh % "hg commit x/aa x/bb .gitignore -m 'First commit'"
sh % "hg status" == r"""
    ? a
    ? b
    ? x/l/aa
    ? x/l/u/a/bb
    ? x/l/u/bb
    ? x/m/aa
    ? x/n/aa
    ? y/l
    ? y/m"""
sh % "hg status --terse mardu" == r"""
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/"""

# Modifying already committed files

sh % "echo Hello" >> "x/aa"
sh % "echo World" >> "x/bb"
sh % "hg status --terse maurdc" == r"""
    M x/aa
    M x/bb
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/"""

# Respecting other flags

sh % "hg status --terse marduic --all" == r"""
    M x/aa
    M x/bb
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/l
    ? y/m
    I x/aa.o
    I x/bb.o
    I y/l.o
    I y/m.o
    C .gitignore"""
sh % "hg status --terse marduic -a"
sh % "hg status --terse marduic -c" == "C .gitignore"
sh % "hg status --terse marduic -m" == r"""
    M x/aa
    M x/bb"""

# Passing 'i' in terse value will consider the ignored files while tersing

sh % "hg status --terse marduic -u" == r"""
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/l
    ? y/m"""

# Omitting 'i' in terse value does not consider ignored files while tersing

sh % "hg status --terse marduc -u" == r"""
    ? a
    ? b
    ? x/l/
    ? x/m/
    ? x/n/
    ? y/"""

# Trying with --rev

sh % "hg status --terse marduic --rev 0 --rev 1" == r"""
    abort: cannot use --terse with --rev
    [255]"""
