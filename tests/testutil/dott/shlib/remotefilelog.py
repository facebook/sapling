# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# Ported from tests/library.sh

from __future__ import absolute_import

import os

from .. import shlib, testtmp


open(testtmp.HGRCPATH, "a").write(
    r"""
# Written by importing dott.sources.remotefilelog
[remotefilelog]
cachepath=$TESTTMP/hgcache
debug=True
historypackv1=True
[extensions]
remotefilelog=
rebase=

[ui]
ssh=python "$TESTDIR/dummyssh"
[server]
preferuncompressed=True

[experimental]
changegroup3=True

[rebase]
singletransaction=True
"""
)


def hgcloneshallow(orig, dest, *args):
    result = shlib.hg(
        "clone",
        "--shallow",
        "--config=remotefilelog.reponame=master",
        orig,
        dest,
        *args
    )
    open(os.path.join(dest, ".hg/hgrc"), "ab").write(
        r"""
[remotefilelog]
reponame=master
[phases]
publish=False
"""
    )
    return result


def hgcloneshallowlfs(orig, dest, lfsdir, *args):
    result = shlib.hg(
        "clone",
        "--shallow",
        "--config=extensions.lfs=",
        "--config=remotefilelog.reponame=master",
        orig,
        dest,
        *args
    )
    open(os.path.join(dest, ".hg/hgrc"), "ab").write(
        r"""
[extensions]
lfs=
[lfs]
url=%s
[remotefilelog]
reponame=master
[phases]
publish=False
"""
        % lfsdir
    )
    return result


def hginit(*args):
    return shlib.hg("init", "--config=remotefilelog.reponame=master", *args)


def clearcache():
    cachepath = os.path.join(testtmp.TESTTMP, "hgcache")
    shlib.rm(cachepath)


def mkcommit(name):
    open(name, "wb").write("%s\n" % name)
    shlib.hg("ci", "-A", name, "-m", name)
