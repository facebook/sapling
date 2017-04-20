# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import gzip
import os
import re

from mercurial.i18n import _
from mercurial.node import nullid, short, hex
from mercurial import (
    error,
    util,
)

from . import p4
from .util import localpath, caseconflict

class ImportSet(object):
    def __init__(self, changelists, filelist, storagepath):
        self.changelists = sorted(changelists)
        self.filelist = filelist
        self.storagepath = storagepath

    def linkrev(self, cl):
        return self._linkrevmap[cl]

    @util.propertycache
    def _linkrevmap(self):
        return {c.cl: idx for idx, c in enumerate(self.changelists)}

    @util.propertycache
    def caseconflicts(self):
        return caseconflict(self.filelist)

    def filelogs(self):
        return list(p4.parse_filelogs(self.changelists, self.filelist))

class ChangeManifestImporter(object):
    def __init__(self, ui, repo, importset):
        self._ui = ui
        self._repo = repo
        self._importset = importset

    @util.propertycache
    def usermap(self):
        m = {}
        for user in p4.parse_usermap():
            m[user['User']] = '%s <%s>' % (user['FullName'], user['Email'])
        return m

    def create(self, tr, fileflags):
        revnumdict = collections.defaultdict(lambda: 0)
        mrevlog = self._repo.manifestlog._revlog
        cp1, cp2 = nullid, nullid
        clog = self._repo.changelog
        p2 = self._repo[cp2]
        for i, change in enumerate(self._importset.changelists):
            # invalidate caches so that the lookup works
            p1 = self._repo[cp1]
            mf = p1.manifest().copy()
            self._ui.progress(_('importing change'), pos=i, item=change,
                    unit='changes', total=len(self._importset.changelists))

            added, modified, removed = change.files

            # generate manifest mappings of filenames to filenodes
            rmod = filter(lambda f: f in self._importset.filelist, removed)
            rf = map(localpath, rmod)
            for path in rf:
                if path in mf:
                    del mf[path]

            addmod = filter(lambda f: f in self._importset.filelist,
                    added + modified)
            amf = map(localpath, addmod)
            for path in amf:
                filelog = self._repo.file(path)
                try:
                    fnode = filelog.node(revnumdict[path])
                except (error.LookupError, IndexError):
                    raise error.Abort("can't find rev %d for %s cl %d" %
                            (revnumdict[path], path, change.cl))
                revnumdict[path] += 1
                mf[path] = fnode
                if path in fileflags and change.cl in fileflags[path]:
                    mf.setflag(path, fileflags[path][change.cl])

            linkrev = self._importset.linkrev(change.cl)
            mp1 = mrevlog.addrevision(mf.text(mrevlog._usemanifestv2), tr,
                                      linkrev,
                                      p1.manifestnode(),
                                      p2.manifestnode())
            self._ui.debug('changelist %d: writing manifest. '
                'node: %s p1: %s p2: %s linkrev: %d\n' % (
                change.cl, short(mp1), short(p1.manifestnode()),
                short(p2.manifestnode()), linkrev))

            desc = change.parsed['desc']
            if desc == '':
                desc = '** empty changelist description **'
            desc = desc.decode('ascii', 'ignore')

            shortdesc = desc.splitlines()[0]
            username = change.parsed['user']
            self._ui.debug('changelist %d: writing changelog: %s\n' % (
                change.cl, shortdesc))
            cp1 = clog.add(
                    mp1,
                    amf + rf,
                    desc,
                    tr,
                    cp1,
                    cp2,
                    user=username,
                    date=(change.parsed['time'], 0),
                    extra={'p4changelist': change.cl})
        self._ui.progress(_('importing change'), pos=None)

class RCSImporter(collections.Mapping):
    def __init__(self, path):
        self._path = path

    @property
    def rcspath(self):
        return '%s,v' % self._path

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
            cmd = 'co -q -p1.%d %s' % (rev, util.shellquote(self.rcspath))
            with util.popen(cmd, mode='rb') as fp:
                text = fp.read()
        return text

    @util.propertycache
    def revisions(self):
        revs = set()
        if os.path.isfile(self.rcspath):
            stdout = util.popen('rlog %s' % util.shellquote(self.rcspath),
                        mode='rb')
            for l in stdout.readlines():
                m = re.match('revision 1.(\d+)', l)
                if m:
                    revs.add(int(m.group(1)))
        return revs

T_FLAT, T_GZIP = 1, 2

class FlatfileImporter(collections.Mapping):
    def __init__(self, path):
        self._path = path

    @property
    def dirpath(self):
        return '%s,d' % self._path

    def __len__(self):
        return len(self.revisions)

    def __iter__(self):
        for r in self.revisions:
            yield r, self[r]

    def filepath(self, rev):
        flat = '%s/1.%d' % (self.dirpath, rev)
        gzip = '%s/1.%d.gz' % (self.dirpath, rev)
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
                revs.add(int(name.split('.')[1]))
        return revs

    def content(self, rev):
        path, type = self.filepath(rev)
        text = None
        if type == T_GZIP:
            with gzip.open(path, 'rb') as fp:
                text = fp.read()
        if type == T_FLAT:
            with open(path, 'rb') as fp:
                text = fp.read()
        return text

class P4FileImporter(collections.Mapping):
    """Read a file from Perforce in case we cannot find it locally, in
    particular when there was branch or a rename"""
    def __init__(self, filelog):
        self._filelog = filelog # type: p4.P4Filelog

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
        return self._filelog.revisions

    def content(self, clnum):
        return p4.get_file(self._filelog.depotfile, clnum=clnum)

class CopyTracer(object):
    def __init__(self, repo, filelist, depotname):
        self._repo = repo
        self._filelist = filelist
        self._depotpath = depotname

    def iscopy(self, cl):
        bcl, bsrc = self.dependency
        return bcl is not None and bcl == cl

    def copydata(self, cl):
        meta = {}
        bcl, bsrc = self.dependency
        if bcl is not None and bcl == cl:
            assert False
            p4fi = P4FileImporter(self._depotpath)
            copylog = self._repo.file(localpath(bsrc))
# XXX: This is most likely broken, as we don't take add->delete->add into
# account
            copynode = copylog.node(p4fi.filelog.branchrev - 1)
            meta["copy"] = localpath(bsrc)
            meta["copyrev"] = hex(copynode)
        return meta

    @util.propertycache
    def dependency(self):
        """Returns a tuple. First value is the cl number when the file was
        branched, the second parameter is the file it was branchedfrom. Other
        otherwise it returns (None, None)
        """
        filelog = p4.parse_filelog(self._depotpath)
        bcl = filelog.branchcl
        bsrc = filelog.branchsource
        if bcl is not None and bsrc in self._filelist:
            return filelog.branchcl, filelog.branchsource
        return None, None

class FileImporter(object):
    def __init__(self, ui, repo, importset, filelog):
        self._ui = ui
        self._repo = repo
        self._i = importset
        self._filelog = filelog # type: p4.P4Filelog

    @property
    def relpath(self):
        # XXX: Do the correct mapping to the clientspec
        return localpath(self._filelog.depotfile)

    @util.propertycache
    def storepath(self):
        path = os.path.join(self._i.storagepath, self.relpath)
        if p4.config('caseHandling') == 'insensitive':
            return path.lower()
        return path

    def create(self, tr, copy_tracer=None):
        assert tr is not None
        p4fi = P4FileImporter(self._filelog)
        rcs = RCSImporter(self.storepath)
        flat = FlatfileImporter(self.storepath)
        local_revs = rcs.revisions | flat.revisions

        revs = []
        for c in self._i.changelists:
            if c.cl in p4fi.revisions and not self._filelog.isdeleted(c.cl):
                revs.append(c)

        fileflags = collections.defaultdict(dict)
        lastlinkrev = 0
        for c in sorted(revs):
            linkrev = self._i.linkrev(c.cl)
            fparent1, fparent2 = nullid, nullid

            # invariant: our linkrevs do not criss-cross.
            assert linkrev >= lastlinkrev
            lastlinkrev = linkrev

            filelog = self._repo.file(self.relpath)
            if len(filelog) > 0:
                fparent1 = filelog.tip()

            # select the content
            text = None
            if c.origcl in local_revs:
                if c.origcl in rcs.revisions:
                    text, src = rcs.content(c.origcl), 'rcs'
                elif c.origcl in flat.revisions:
                    text, src = flat.content(c.origcl), 'gzip'
            elif c.cl in p4fi.revisions:
                text, src = p4fi.content(c.cl), 'p4'
            if text is None:
                raise error.Abort('error generating file content %d %s' % (
                    c.cl, self.relpath))

            meta = {}
            # iscopy = copy_tracer and copy_tracer.iscopy(c.cl)
            #if iscopy:
            #    meta = copy_tracer.copydata(c.cl)
            if self._filelog.isexec(c.cl):
                fileflags[self.relpath][c.cl] = 'x'
            if self._filelog.issymlink(c.cl):
                fileflags[self.relpath][c.cl] = 'l'
            if self._filelog.iskeyworded(c.cl):
                # Replace keyword expansion
                pass

            h = filelog.add(text, meta, tr, linkrev, fparent1, fparent2)
            self._ui.debug(
                'writing filelog: %s, p1 %s, linkrev %d, %d bytes, src: %s, '
                'path: %s\n' % (short(h), short(fparent1), linkrev,
                    len(text), src, self.relpath))
        return fileflags
