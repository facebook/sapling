# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Tests for the automv extension; detect moved files at commit time.

sh % "cat" << r"""
[extensions]
automv=
rebase=
""" >> "$HGRCPATH"

# Setup repo

sh % "hg init repo"
sh % "cd repo"

# Test automv command for commit

sh % "printf 'foo\\nbar\\nbaz\\n'" > "a.txt"
sh % "hg add a.txt"
sh % "hg commit -m 'init repo with a'"

# mv/rm/add
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit -m msg" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# mv/rm/add/modif
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "printf '\\n'" >> "b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit -m msg" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# mv/rm/add/modif
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "printf '\\nfoo\\n'" >> "b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit -m msg"
sh % "hg status --change . -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# mv/rm/add/modif/changethreshold
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "printf '\\nfoo\\n'" >> "b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --config 'automv.similarity=60' -m msg" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# mv
sh % "mv a.txt b.txt"
sh % "hg status -C" == r"""
    ! a.txt
    ? b.txt"""
sh % "hg commit -m msg" == r"""
    nothing changed (1 missing files, see 'hg status')
    [1]"""
sh % "hg status -C" == r"""
    ! a.txt
    ? b.txt"""
sh % "hg revert -aqC"
sh % "rm b.txt"

# mv/rm/add/notincommitfiles
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "echo bar" > "c.txt"
sh % "hg add c.txt"
sh % "hg status -C" == r"""
    A b.txt
    A c.txt
    R a.txt"""
sh % "hg commit c.txt -m msg"
sh % "hg status --change . -C" == "A c.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg up -r 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg rm a.txt"
sh % "echo bar" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m msg" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    A c.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"

# mv/rm/add/--no-automv
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --no-automv -m msg"
sh % "hg status --change . -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# Test automv command for commit --amend

# mv/rm/add
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --amend -m amended" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    A c.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"

# mv/rm/add/modif
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "printf '\\n'" >> "b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --amend -m amended" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    A c.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"

# mv/rm/add/modif
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "printf '\\nfoo\\n'" >> "b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --amend -m amended"
sh % "hg status --change . -C" == r"""
    A b.txt
    A c.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"

# mv/rm/add/modif/changethreshold
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "printf '\\nfoo\\n'" >> "b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --amend --config 'automv.similarity=60' -m amended" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    A c.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"

# mv
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg status -C" == r"""
    ! a.txt
    ? b.txt"""
sh % "hg commit --amend -m amended"
sh % "hg status -C" == r"""
    ! a.txt
    ? b.txt"""
sh % "hg up -Cr 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

# mv/rm/add/notincommitfiles
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "echo bar" > "d.txt"
sh % "hg add d.txt"
sh % "hg status -C" == r"""
    A b.txt
    A d.txt
    R a.txt"""
sh % "hg commit --amend -m amended d.txt"
sh % "hg status --change . -C" == r"""
    A c.txt
    A d.txt"""
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --amend -m amended" == "detected move of 1 files"
sh % "hg status --change . -C" == r"""
    A b.txt
      a.txt
    A c.txt
    A d.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 3 files removed, 0 files unresolved"

# mv/rm/add/--no-automv
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg add b.txt"
sh % "hg status -C" == r"""
    A b.txt
    R a.txt"""
sh % "hg commit --amend -m amended --no-automv"
sh % "hg status --change . -C" == r"""
    A b.txt
    A c.txt
    R a.txt"""
sh % "hg up -r 0" == "1 files updated, 0 files merged, 2 files removed, 0 files unresolved"

# mv/rm/commit/add/amend
sh % "echo c" > "c.txt"
sh % "hg add c.txt"
sh % "hg commit -m 'revision to amend to'"
sh % "mv a.txt b.txt"
sh % "hg rm a.txt"
sh % "hg status -C" == r"""
    R a.txt
    ? b.txt"""
sh % "hg commit -m 'removed a'"
sh % "hg add b.txt"
sh % "hg commit --amend -m amended"
sh % "hg status --change . -C" == r"""
    A b.txt
    R a.txt"""

# error conditions

sh % "cat" << r"""
[automv]
similarity=110
""" >> "$HGRCPATH"
sh % "hg commit -m 'revision to amend to'" == r"""
    abort: automv.similarity must be between 0 and 100
    [255]"""
