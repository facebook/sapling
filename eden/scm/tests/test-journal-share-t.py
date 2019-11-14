# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Journal extension test: tests the share extension support

sh % "cat" << r"""
# mock out util.getuser() and util.makedate() to supply testable values
import os
from edenscm.mercurial import util
def mockgetuser():
    return 'foobar'

def mockmakedate():
    filename = os.path.join(os.environ['TESTTMP'], 'testtime')
    try:
        with open(filename, 'rb') as timef:
            time = float(timef.read()) + 1
    except IOError:
        time = 0.0
    with open(filename, 'wb') as timef:
        timef.write(str(time))
    return (time, 0)

util.getuser = mockgetuser
util.makedate = mockmakedate
""" >> "testmocks.py"

sh % "cat" << r"""
[extensions]
journal=
share=
testmocks=`pwd`/testmocks.py
[remotenames]
rename.default=remote
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"
sh % "hg bookmark bm"
sh % "touch file0"
sh % "hg commit -Am file0-added" == "adding file0"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         commit -Am file0-added
    0fd3805711f9  bm        commit -Am file0-added"""

# A shared working copy initially receives the same bookmarks and working copy

sh % "cd .."
sh % "hg share repo shared1" == r"""
    updating working directory
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd shared1"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         share repo shared1"""

# unless you explicitly share bookmarks

sh % "cd .."
sh % "hg share --bookmarks repo shared2" == r"""
    updating working directory
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd shared2"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         share --bookmarks repo shared2
    0fd3805711f9  bm        commit -Am file0-added"""

# Moving the bookmark in the original repository is only shown in the repository
# that shares bookmarks

sh % "cd ../repo"
sh % "touch file1"
sh % "hg commit -Am file1-added" == "adding file1"
sh % "cd ../shared1"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         share repo shared1"""
sh % "cd ../shared2"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    4f354088b094  bm        commit -Am file1-added
    0fd3805711f9  .         share --bookmarks repo shared2
    0fd3805711f9  bm        commit -Am file0-added"""

# But working copy changes are always 'local'

sh % "cd ../repo"
sh % "hg up 0" == r"""
    0 files updated, 0 files merged, 1 files removed, 0 files unresolved
    (leaving bookmark bm)"""
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         up 0
    4f354088b094  .         commit -Am file1-added
    4f354088b094  bm        commit -Am file1-added
    0fd3805711f9  .         commit -Am file0-added
    0fd3805711f9  bm        commit -Am file0-added"""
sh % "cd ../shared2"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    4f354088b094  bm        commit -Am file1-added
    0fd3805711f9  .         share --bookmarks repo shared2
    0fd3805711f9  bm        commit -Am file0-added"""
sh % "hg up tip" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg up 0" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "hg journal" == r"""
    previous locations of '.':
    0fd3805711f9  up 0
    4f354088b094  up tip
    0fd3805711f9  share --bookmarks repo shared2"""

# Unsharing works as expected; the journal remains consistent

sh % "cd ../shared1"
sh % "hg unshare"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         share repo shared1"""
sh % "cd ../shared2"
sh % "hg unshare"
sh % "hg journal --all" == r"""
    previous locations of the working copy and bookmarks:
    0fd3805711f9  .         up 0
    4f354088b094  .         up tip
    4f354088b094  bm        commit -Am file1-added
    0fd3805711f9  .         share --bookmarks repo shared2
    0fd3805711f9  bm        commit -Am file0-added"""

# New journal entries in the source repo no longer show up in the other working copies

sh % "cd ../repo"
sh % "hg bookmark newbm -r tip"
sh % "hg journal newbm" == r"""
    previous locations of 'newbm':
    4f354088b094  bookmark newbm -r tip"""
sh % "cd ../shared2"
sh % "hg journal newbm" == r"""
    previous locations of 'newbm':
    no recorded locations"""

# This applies for both directions

sh % "hg bookmark shared2bm -r tip"
sh % "hg journal shared2bm" == r"""
    previous locations of 'shared2bm':
    4f354088b094  bookmark shared2bm -r tip"""
sh % "cd ../repo"
sh % "hg journal shared2bm" == r"""
    previous locations of 'shared2bm':
    no recorded locations"""
