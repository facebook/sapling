# (c) 2017-present Facebook Inc.
from __future__ import absolute_import

import collections
import gzip
import os
import re

from mercurial.i18n import _
from mercurial.node import nullid, short
from mercurial import (
    error,
    extensions,
    util,
)

from . import p4
from .util import caseconflict, localpath

KEYWORD_REGEX = "\$(Id|Header|DateTime|" + \
                "Date|Change|File|" + \
                "Revision|Author).*?\$"

class ImportSet(object):
    def __init__(self, repo, client, changelists, filelist, storagepath):
        self.repo = repo
        self.client = client
        self.changelists = sorted(changelists)
        self.filelist = filelist
        self.storagepath = storagepath

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
        for filelog in p4.parse_filelogs(self.repo.ui, self.client,
                                         self.changelists, self.filelist):
            yield filelog

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

    def create(self, *args, **kwargs):
        return list(self.creategen(*args, **kwargs))

    def creategen(self, tr, fileinfo):
        mrevlog = self._repo.manifestlog._revlog
        clog = self._repo.changelog
        cp1 = self._repo['tip'].node()
        cp2 = nullid
        p1 = self._repo[cp1]
        mp1 = p1.manifestnode()
        mp2 = nullid
        mf = p1.manifest().copy()
        for i, change in enumerate(self._importset.changelists):
            self._ui.progress(_('importing change'), pos=i, item=change,
                    unit='changes', total=len(self._importset.changelists))

            added, modified, removed = change.files

            changed = set()

            # generate manifest mappings of filenames to filenodes
            for depotname in removed:
                if depotname not in self._importset.filelist:
                    continue
                localname = fileinfo[depotname]['localname']
                if localname in mf:
                    changed.add(localname)
                    del mf[localname]

            for depotname in added + modified:
                if depotname not in self._importset.filelist:
                    continue
                info = fileinfo[depotname]
                localname, baserev = info['localname'], info['baserev']
                hgfilelog = self._repo.file(localname)
                try:
                    mf[localname] = hgfilelog.node(baserev)
                except (error.LookupError, IndexError):
                    raise error.Abort("can't find rev %d for %s cl %d" % (
                        baserev, localname, change.cl))
                changed.add(localname)
                if change.cl in info['flags']:
                    mf.setflag(localname, info['flags'][change.cl])
                fileinfo[depotname]['baserev'] += 1

            linkrev = self._importset.linkrev(change.cl)
            oldmp1 = mp1
            mp1 = mrevlog.addrevision(mf.text(mrevlog._usemanifestv2), tr,
                                      linkrev, mp1, mp2)
            self._ui.debug('changelist %d: writing manifest. '
                'node: %s p1: %s p2: %s linkrev: %d\n' % (
                change.cl, short(mp1), short(oldmp1), short(mp2), linkrev))

            desc = change.parsed['desc']
            if desc == '':
                desc = '** empty changelist description **'
            desc = desc.decode('ascii', 'ignore')

            shortdesc = desc.splitlines()[0]
            username = change.parsed['user']
            username = self.usermap.get(username, username)
            self._ui.debug('changelist %d: writing changelog: %s\n' % (
                change.cl, shortdesc))
            cp1 = self.writechangelog(
                    clog, mp1, changed, desc, tr, cp1, cp2,
                    username, (change.parsed['time'], 0), change.cl)
            yield change.cl, cp1
        self._ui.progress(_('importing change'), pos=None)

    def writechangelog(
            self, clog, mp1, changed, desc, tr, cp1, cp2, username, date, cl):
        return clog.add(
                mp1, changed, desc, tr, cp1, cp2,
                user=username, date=date,
                extra={'p4changelist': cl})

class BlobChangeManifestImporter(ChangeManifestImporter):
    def writechangelog(
            self, clog, mp1, changed, desc, tr, cp1, cp2, username, date, cl):
        return clog.add(
                mp1, changed, desc, tr, cp1, cp2,
                user=username, date=date,
                extra={'p4fullimportbasechangelist': cl})

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
            cmd = 'co -kk -q -p1.%d %s' % (rev, util.shellquote(self.rcspath))
            with util.popen(cmd, mode='rb') as fp:
                text = fp.read()
        return text

    @util.propertycache
    def revisions(self):
        revs = set()
        if os.path.isfile(self.rcspath):
            stdout = util.popen('rlog %s 2>%s'
                                % (util.shellquote(self.rcspath), os.devnull),
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
    def __init__(self, p4filelog):
        self._p4filelog = p4filelog # type: p4.P4Filelog

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
    def __init__(self, ui, repo, importset, p4filelog):
        self._ui = ui
        self._repo = repo
        self._importset = importset
        self._p4filelog = p4filelog # type: p4.P4Filelog

    @util.propertycache
    def relpath(self):
        client = self._importset.client
        where = p4.parse_where(client, self.depotfile)
        filename = where['clientFile'].replace('//%s/' % client, '')
        return p4.decodefilename(filename)

    @property
    def depotfile(self):
        return self._p4filelog.depotfile

    @util.propertycache
    def storepath(self):
        path = os.path.join(self._importset.storagepath,
                localpath(self.depotfile))
        if p4.config('caseHandling') == 'insensitive':
            return path.lower()
        return path

    def hgfilelog(self):
        return self._repo.file(self.relpath)

    def findlfs(self):
        try:
            return extensions.find('lfs')
        except KeyError:
            pass
        return None

    def create(self, tr, copy_tracer=None):
        assert tr is not None
        p4fi = P4FileImporter(self._p4filelog)
        rcs = RCSImporter(self.storepath)
        flat = FlatfileImporter(self.storepath)
        local_revs = rcs.revisions | flat.revisions

        revs = set()
        for c in self._importset.changelists:
            if c.cl in p4fi.revisions and not self._p4filelog.isdeleted(c.cl):
                revs.add(c)

        fileflags = collections.defaultdict(dict)
        lastlinkrev = 0

        hgfilelog = self.hgfilelog()
        origlen = len(hgfilelog)
        largefiles = []
        lfsext = self.findlfs()
        for c in sorted(revs):
            linkrev = self._importset.linkrev(c.cl)
            fparent1, fparent2 = nullid, nullid

            # invariant: our linkrevs do not criss-cross. They are monotonically
            # increasing. See https://www.mercurial-scm.org/wiki/CrossedLinkrevs
            assert linkrev >= lastlinkrev
            lastlinkrev = linkrev

            if len(hgfilelog) > 0:
                fparent1 = hgfilelog.tip()

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
            if self._p4filelog.isexec(c.cl):
                fileflags[c.cl] = 'x'
            if self._p4filelog.issymlink(c.cl):
                # On Linux/Unix a symlink must not have a newline. Perforce
                # however returns a newline at the end which we must strip.
                text = text.rstrip()
                fileflags[c.cl] = 'l'
            if self._p4filelog.iskeyworded(c.cl):
                text = re.sub(KEYWORD_REGEX, r'$\1$', text)

            node = hgfilelog.add(text, meta, tr, linkrev, fparent1, fparent2)
            self._ui.debug(
                'writing filelog: %s, p1 %s, linkrev %d, %d bytes, src: %s, '
                'path: %s\n' % (short(node), short(fparent1), linkrev,
                    len(text), src, self.relpath))

            if lfsext and lfsext.wrapper._islfs(hgfilelog, node):
                lfspointer = lfsext.pointer.deserialize(
                        hgfilelog.revision(node, raw=True))
                oid = lfspointer.oid()
                largefiles.append((c.cl, self.depotfile, oid))
                self._ui.debug('largefile: %s, oid: %s\n' % (self.relpath, oid))

        newlen = len(hgfilelog)
        return fileflags, largefiles, origlen, newlen

class BlobFileImporter(FileImporter):
    def create(self, tr):
        assert tr is not None
        p4fi = P4FileImporter(self._p4filelog)
        revs = set()
        for c in self._importset.changelists:
            if c.cl in p4fi.revisions and not self._p4filelog.isdeleted(c.cl):
                revs.add(c)

        fileflags = collections.defaultdict(dict)
        lastlinkrev = 0

        hgfilelog = self.hgfilelog()
        origlen = len(hgfilelog)
        largefiles = []
        lfsext = self.findlfs()

        for c in sorted(revs):
            linkrev = self._importset.linkrev(c.cl)
            fparent1, fparent2 = nullid, nullid

            assert linkrev >= lastlinkrev
            lastlinkrev = linkrev

            if len(hgfilelog) > 0:
                fparent1 = hgfilelog.tip()

            text = None
            # Only read files from p4 for a blob commit
            text, src = p4fi.content(c.cl), 'p4'
            if text is None:
                raise error.Abort('error generating file content %d %s' % (
                    c.cl, self.relpath))

            meta = {}
            if self._p4filelog.isexec(c.cl):
                fileflags[c.cl] = 'x'
            if self._p4filelog.issymlink(c.cl):
                fileflags[c.cl] = 'l'
            if self._p4filelog.iskeyworded(c.cl):
                text = re.sub(KEYWORD_REGEX, r'$\1$', text)

            node = hgfilelog.add(text, meta, tr, linkrev, fparent1, fparent2)
            self._ui.debug(
                'writing filelog: %s, p1 %s, linkrev %d, %d bytes, src: %s, '
                'path: %s\n' % (short(node), short(fparent1), linkrev,
                    len(text), src, self.relpath))

            if lfsext and lfsext.wrapper._islfs(hgfilelog, node):
                lfspointer = lfsext.pointer.deserialize(
                        hgfilelog.revision(node, raw=True))
                oid = lfspointer.oid()
                largefiles.append((c.cl, self.depotfile, oid))
                self._ui.debug('largefile: %s, oid: %s\n' % (self.relpath, oid))

        newlen = len(hgfilelog)
        return fileflags, largefiles, origlen, newlen
