# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import gzip
import os
import re

from edenscm.mercurial import error, util

from . import p4
from .util import localpath


KEYWORD_REGEX = "\$(Id|Header|DateTime|" + "Date|Change|File|" + "Revision|Author).*?\$"

# TODO: make p4 user configurable
P4_ADMIN_USER = "p4admin"


def get_p4_file_content(storepath, p4filelog, p4cl, skipp4revcheck=False):
    p4path = p4filelog._depotfile
    p4storepath = os.path.join(storepath, localpath(p4path))
    if p4.config("caseHandling") == "insensitive":
        p4storepath = p4storepath.lower()

    rcs = RCSImporter(p4storepath)
    if p4cl.origcl in rcs.revisions:
        return rcs.content(p4cl.origcl), "rcs"

    flat = FlatfileImporter(p4storepath)
    if p4cl.origcl in flat.revisions:
        return flat.content(p4cl.origcl), "gzip"

    # This is needed when reading a file from p4 during sync import:
    # when sync import constructs a filelog, it uses "latestcl" as the key
    # instead of "headcl", so the check for whether p4cl.cl is inside
    # p4fi.revisions will fail, and not necessary
    if skipp4revcheck:
        return p4.get_file(p4filelog.depotfile, clnum=p4cl.cl), "p4"
    p4fi = P4FileImporter(p4filelog)
    if p4cl.cl in p4fi.revisions:
        return p4fi.content(p4cl.cl), "p4"
    raise error.Abort("error generating file content %d %s" % (p4cl.cl, p4path))


class RCSImporter(collections.Mapping):
    def __init__(self, path):
        self._path = path

    @property
    def rcspath(self):
        return "%s,v" % self._path

    def __getitem__(self, rev):
        if rev in self.revisions:
            return self.content(rev)
        return IndexError

    def __len__(self):
        return len(self.revisions)

    def __iter__(self):
        for r in self.revisions:
            yield r, self[r]

    def content(self, rev):
        text = None
        if os.path.isfile(self.rcspath):
            cmd = "co -kk -q -p1.%d %s" % (rev, util.shellquote(self.rcspath))
            with util.popen(cmd, mode="rb") as fp:
                text = fp.read()
        return text

    @util.propertycache
    def revisions(self):
        revs = set()
        if os.path.isfile(self.rcspath):
            stdout = util.popen(
                "rlog %s 2>%s" % (util.shellquote(self.rcspath), os.devnull), mode="rb"
            )
            for l in stdout.readlines():
                m = re.match("revision 1.(\d+)", l)
                if m:
                    revs.add(int(m.group(1)))
        return revs


T_FLAT, T_GZIP = 1, 2


class FlatfileImporter(collections.Mapping):
    def __init__(self, path):
        self._path = path

    @property
    def dirpath(self):
        return "%s,d" % self._path

    def __len__(self):
        return len(self.revisions)

    def __iter__(self):
        for r in self.revisions:
            yield r, self[r]

    def filepath(self, rev):
        flat = "%s/1.%d" % (self.dirpath, rev)
        gzip = "%s/1.%d.gz" % (self.dirpath, rev)
        if os.path.exists(flat):
            return flat, T_FLAT
        if os.path.exists(gzip):
            return gzip, T_GZIP
        return None, None

    def __getitem__(self, rev):
        text = self.content(rev)
        if text is None:
            raise IndexError
        return text

    @util.propertycache
    def revisions(self):
        revs = set()
        if os.path.isdir(self.dirpath):
            for name in os.listdir(self.dirpath):
                revs.add(int(name.split(".")[1]))
        return revs

    def content(self, rev):
        path, type = self.filepath(rev)
        text = None
        if type == T_GZIP:
            with gzip.open(path, "rb") as fp:
                text = fp.read()
        if type == T_FLAT:
            with open(path, "rb") as fp:
                text = fp.read()
        return text


class P4FileImporter(collections.Mapping):
    """Read a file from Perforce in case we cannot find it locally, in
    particular when there was branch or a rename"""

    def __init__(self, p4filelog):
        self._p4filelog = p4filelog  # type: p4.P4Filelog

    def __len__(self):
        return len(self.revisions)

    def __iter__(self):
        for r in self.revisions:
            yield r, self[r]

    def __getitem__(self, rev):
        text = self.content(rev)
        if text is None:
            raise IndexError
        return text

    @util.propertycache
    def revisions(self):
        return self._p4filelog.revisions

    def content(self, clnum):
        return p4.get_file(self._p4filelog.depotfile, clnum=clnum)
