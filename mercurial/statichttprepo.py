# statichttprepo.py - simple http repository class for mercurial
#
# This provides read-only repo access to repositories exported via static http
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import changelog, byterange, url, error, namespaces
import localrepo, manifest, util, scmutil, store
import urllib, urllib2, errno, os

class httprangereader(object):
    def __init__(self, url, opener):
        # we assume opener has HTTPRangeHandler
        self.url = url
        self.pos = 0
        self.opener = opener
        self.name = url
    def seek(self, pos):
        self.pos = pos
    def read(self, bytes=None):
        req = urllib2.Request(self.url)
        end = ''
        if bytes:
            end = self.pos + bytes - 1
        if self.pos or end:
            req.add_header('Range', 'bytes=%d-%s' % (self.pos, end))

        try:
            f = self.opener.open(req)
            data = f.read()
            code = f.code
        except urllib2.HTTPError as inst:
            num = inst.code == 404 and errno.ENOENT or None
            raise IOError(num, inst)
        except urllib2.URLError as inst:
            raise IOError(None, inst.reason[1])

        if code == 200:
            # HTTPRangeHandler does nothing if remote does not support
            # Range headers and returns the full entity. Let's slice it.
            if bytes:
                data = data[self.pos:self.pos + bytes]
            else:
                data = data[self.pos:]
        elif bytes:
            data = data[:bytes]
        self.pos += len(data)
        return data
    def readlines(self):
        return self.read().splitlines(True)
    def __iter__(self):
        return iter(self.readlines())
    def close(self):
        pass

def build_opener(ui, authinfo):
    # urllib cannot handle URLs with embedded user or passwd
    urlopener = url.opener(ui, authinfo)
    urlopener.add_handler(byterange.HTTPRangeHandler())

    class statichttpvfs(scmutil.abstractvfs):
        def __init__(self, base):
            self.base = base

        def __call__(self, path, mode='r', *args, **kw):
            if mode not in ('r', 'rb'):
                raise IOError('Permission denied')
            f = "/".join((self.base, urllib.quote(path)))
            return httprangereader(f, urlopener)

        def join(self, path):
            if path:
                return os.path.join(self.base, path)
            else:
                return self.base

    return statichttpvfs

class statichttppeer(localrepo.localpeer):
    def local(self):
        return None
    def canpush(self):
        return False

class statichttprepository(localrepo.localrepository):
    supported = localrepo.localrepository._basesupported

    def __init__(self, ui, path):
        self._url = path
        self.ui = ui

        self.root = path
        u = util.url(path.rstrip('/') + "/.hg")
        self.path, authinfo = u.authinfo()

        opener = build_opener(ui, authinfo)
        self.opener = opener(self.path)
        self.vfs = self.opener
        self._phasedefaults = []

        self.names = namespaces.namespaces()

        try:
            requirements = scmutil.readrequires(self.vfs, self.supported)
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            requirements = set()

            # check if it is a non-empty old-style repository
            try:
                fp = self.vfs("00changelog.i")
                fp.read(1)
                fp.close()
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise
                # we do not care about empty old-style repositories here
                msg = _("'%s' does not appear to be an hg repository") % path
                raise error.RepoError(msg)

        # setup store
        self.store = store.store(requirements, self.path, opener)
        self.spath = self.store.path
        self.svfs = self.store.opener
        self.sjoin = self.store.join
        self._filecache = {}
        self.requirements = requirements

        self.manifest = manifest.manifest(self.svfs)
        self.changelog = changelog.changelog(self.svfs)
        self._tags = None
        self.nodetagscache = None
        self._branchcaches = {}
        self._revbranchcache = None
        self.encodepats = None
        self.decodepats = None
        self._transref = None

    def _restrictcapabilities(self, caps):
        caps = super(statichttprepository, self)._restrictcapabilities(caps)
        return caps.difference(["pushkey"])

    def url(self):
        return self._url

    def local(self):
        return False

    def peer(self):
        return statichttppeer(self)

    def lock(self, wait=True):
        raise util.Abort(_('cannot lock static-http repository'))

def instance(ui, path, create):
    if create:
        raise util.Abort(_('cannot create new static-http repository'))
    return statichttprepository(ui, path[7:])
