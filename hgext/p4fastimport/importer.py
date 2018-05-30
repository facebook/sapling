# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import gzip
import os
import re

from mercurial.i18n import _
from mercurial.node import nullid, short
from mercurial import error, manifest, progress, util

from . import lfs, p4
from .util import caseconflict, localpath

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


class ImportSet(object):
    def __init__(
        self, repo, client, changelists, filelist, storagepath, isbranchpoint=False
    ):
        self.repo = repo
        self.client = client
        self.changelists = sorted(changelists)
        self.filelist = filelist
        self.storagepath = storagepath
        self.isbranchpoint = isbranchpoint

    def linkrev(self, cl):
        return self._linkrevmap[cl]

    @util.propertycache
    def _linkrevmap(self):
        start = len(self.repo)
        return {c.cl: idx + start for idx, c in enumerate(self.changelists)}

    @util.propertycache
    def caseconflicts(self):
        return caseconflict(self.filelist)

    def filelogs(self):
        for filelog in p4.parse_filelogs(
            self.repo.ui, self.client, self.changelists, self.filelist
        ):
            yield filelog


class ChangeManifestImporter(object):
    def __init__(self, ui, repo, importset, p1ctx):
        self._ui = ui
        self._repo = repo
        self._importset = importset
        self._p1ctx = p1ctx

    def create(self, *args, **kwargs):
        return list(self.creategen(*args, **kwargs))

    def creategen(self, tr, fileinfo):
        mrevlog = self._repo.manifestlog._revlog
        clog = self._repo.changelog
        cp1 = self._p1ctx.node()
        cp2 = nullid
        p1 = self._repo[cp1]
        mp1 = p1.manifestnode()
        mp2 = nullid
        if self._importset.isbranchpoint:
            mf = manifest.manifestdict()
        else:
            mf = p1.manifest().copy()
        with progress.bar(
            self._ui, _("importing change"), "changes", len(self._importset.changelists)
        ) as prog:
            for i, change in enumerate(self._importset.changelists):
                prog.value = (i, change)

                added, modified, removed = change.files

                changed = set()

                # generate manifest mappings of filenames to filenodes
                for depotname in removed:
                    if depotname not in self._importset.filelist:
                        continue
                    localname = fileinfo[depotname]["localname"]
                    if localname in mf:
                        changed.add(localname)
                        del mf[localname]

                for depotname in added + modified:
                    if depotname not in self._importset.filelist:
                        continue
                    info = fileinfo[depotname]
                    localname = info["localname"]
                    baserev = info["baserevatcl"][str(change.cl)]

                    if self._ui.configbool("p4fastimport", "checksymlinks", True):
                        # Under rare situations, when a symlink points to a
                        # directory, the P4 server can report a file "under" it
                        # (as if it really were a directory). 'p4 sync' reports
                        # this as an error and continues, but 'hg update' will
                        # abort if it encounters this.  We need to keep such
                        # damage out of the hg repository.
                        depotparentname = os.path.dirname(depotname)

                        # The manifest's flags for the parent haven't been
                        # updated to reflect this changelist yet. If the
                        # parent's flags are changing right now, use them.
                        # Otherwise, use the manifest's flags.
                        parentflags = None
                        parentinfo = fileinfo.get(depotparentname, None)
                        if parentinfo:
                            parentflags = parentinfo["flags"].get(change.cl, None)

                        localparentname = localname
                        while parentflags is None:
                            # This P4 commit didn't change parent's flags at
                            # all. Therefore, we can consult the Hg metadata.
                            localparentname = os.path.dirname(localparentname)
                            if localparentname == "":
                                # There was no parent file above localname; only
                                # directories. That's good/expected.
                                parentflags = ""
                                break
                            parentflags = mf.flags(localparentname, None)

                        if "l" in parentflags:
                            # It turns out that some parent is a symlink, so
                            # this file can't exist. However, we already wrote
                            # the filelog! Oh well. Just don't reference it in
                            # the manifest.
                            # TODO: hgfilelog.strip()?
                            msg = _(
                                "warning: ignoring {} because it's under a "
                                "symlink ({})\n"
                            ).format(localname, localparentname)
                            self._ui.warn(msg)
                            continue

                    hgfilelog = self._repo.file(localname)
                    try:
                        mf[localname] = hgfilelog.node(baserev)
                    except (error.LookupError, IndexError):
                        raise error.Abort(
                            "can't find rev %d for %s cl %d"
                            % (baserev, localname, change.cl)
                        )
                    changed.add(localname)
                    flags = info["flags"].get(change.cl, "")
                    if flags != mf.flags(localname):
                        mf.setflag(localname, flags)

                linkrev = self._importset.linkrev(change.cl)
                oldmp1 = mp1
                mp1 = mrevlog.addrevision(
                    mf.text(mrevlog._usemanifestv2), tr, linkrev, mp1, mp2
                )
                self._ui.debug(
                    "changelist %d: writing manifest. "
                    "node: %s p1: %s p2: %s linkrev: %d\n"
                    % (change.cl, short(mp1), short(oldmp1), short(mp2), linkrev)
                )

                desc = change.description
                shortdesc = desc.splitlines()[0]
                self._ui.debug(
                    "changelist %d: writing changelog: %s\n" % (change.cl, shortdesc)
                )
                cp1 = self.writechangelog(
                    clog,
                    mp1,
                    changed,
                    desc,
                    tr,
                    cp1,
                    cp2,
                    change.user,
                    change.hgdate,
                    change.cl,
                )
                yield change.cl, cp1

    def writechangelog(
        self, clog, mp1, changed, desc, tr, cp1, cp2, username, date, cl
    ):
        return clog.add(
            mp1,
            changed,
            desc,
            tr,
            cp1,
            cp2,
            user=username,
            date=date,
            extra={"p4changelist": cl},
        )


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


class FileImporter(object):
    def __init__(self, ui, repo, importset, p4filelog, p1ctx):
        self._ui = ui
        self._repo = repo
        self._importset = importset
        self._p4filelog = p4filelog  # type: p4.P4Filelog
        self._p1ctx = p1ctx

    @util.propertycache
    def relpath(self):
        return p4.parse_where(self._importset.client, self.depotfile)

    @property
    def depotfile(self):
        return self._p4filelog.depotfile

    @util.propertycache
    def storepath(self):
        path = os.path.join(self._importset.storagepath, localpath(self.depotfile))
        if p4.config("caseHandling") == "insensitive":
            return path.lower()
        return path

    def hgfilelog(self):
        return self._repo.file(self.relpath)

    def create(self, tr, copy_tracer=None):
        assert tr is not None
        p4fi = P4FileImporter(self._p4filelog)
        rcs = RCSImporter(self.storepath)
        flat = FlatfileImporter(self.storepath)
        local_revs = rcs.revisions | flat.revisions

        revs = set()
        for c in self._importset.changelists:
            if c.cl in p4fi.revisions:
                revs.add(c)

        hgfile = self.relpath
        p1 = self._p1ctx
        try:
            fnode = p1[hgfile].filenode()
            wasdeleted = False
        except error.ManifestLookupError:
            # file doesn't exist in p1
            fnode = nullid
            wasdeleted = True

        baserevatcl = collections.defaultdict(dict)
        fileflags = collections.defaultdict(dict)
        lastlinkrev = 0

        hgfilelog = self.hgfilelog()
        origlen = len(hgfilelog)
        largefiles = []
        for c in sorted(revs):
            if self._p4filelog.isdeleted(c.cl):
                wasdeleted = True
                continue

            linkrev = self._importset.linkrev(c.cl)
            fparent1, fparent2 = nullid, nullid

            # invariant: our linkrevs do not criss-cross. They are monotonically
            # increasing. See https://www.mercurial-scm.org/wiki/CrossedLinkrevs
            assert linkrev >= lastlinkrev
            lastlinkrev = linkrev

            if wasdeleted is False:
                fparent1 = fnode
            wasdeleted = False

            # select the content
            text = None
            if c.origcl in local_revs:
                if c.origcl in rcs.revisions:
                    text, src = rcs.content(c.origcl), "rcs"
                elif c.origcl in flat.revisions:
                    text, src = flat.content(c.origcl), "gzip"
            elif c.cl in p4fi.revisions:
                text, src = p4fi.content(c.cl), "p4"
            if text is None:
                raise error.Abort(
                    "error generating file content %d %s" % (c.cl, self.relpath)
                )

            meta = {}
            if self._p4filelog.isexec(c.cl):
                fileflags[c.cl] = "x"
            if self._p4filelog.issymlink(c.cl):
                # On Linux/Unix a symlink must not have a newline. Perforce
                # however returns a newline at the end which we must strip.
                text = text.rstrip()
                fileflags[c.cl] = "l"
            if self._p4filelog.iskeyworded(c.cl):
                text = re.sub(KEYWORD_REGEX, r"$\1$", text)

            node = hgfilelog.add(text, meta, tr, linkrev, fparent1, fparent2)
            self._ui.debug(
                "writing filelog: %s, p1 %s, linkrev %d, %d bytes, src: %s, "
                "path: %s\n"
                % (short(node), short(fparent1), linkrev, len(text), src, self.relpath)
            )

            baserev = len(hgfilelog) - 1
            # abort when filelog is still empty  after writing entries
            if baserev < 0:
                raise error.Abort(
                    "fail to write to hg filelog for file %s at %d" % (hgfile, c.cl)
                )
            baserevatcl[c.cl] = baserev

            islfs, oid = lfs.getlfsinfo(hgfilelog, node)
            if islfs:
                largefiles.append((c.cl, self.depotfile, oid))
                self._ui.debug("largefile: %s, oid: %s\n" % (self.relpath, oid))

            fnode = node

        newlen = len(hgfilelog)
        return fileflags, largefiles, baserevatcl, origlen, newlen
