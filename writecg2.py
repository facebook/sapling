# writecg2.py -- write changegroup2 to disk
#
# Copyright 2004-present Facebook.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''write changegroup2 to disk

For histories with lots of interleaved branches stored with generaldelta,
bundle1 can be extremely slow to generate. This extension modifies Mercurial to
read and write changegroup2s to disk.
'''

from mercurial import bundle2
from mercurial import bundlerepo
from mercurial import changegroup
from mercurial import discovery
from mercurial import error
from mercurial import exchange
from mercurial import extensions
from mercurial import localrepo
from mercurial import phases
from mercurial import util
from mercurial.i18n import _
from mercurial.node import nullid

import os
import tempfile

def overridewritebundle(orig, ui, cg, filename, bundletype, vfs=None):
    if (bundletype.startswith('HG10') and
        isinstance(cg, changegroup.cg2unpacker)):
        bundletype = 'HG2C' + bundletype[4:]
    return orig(ui, cg, filename, bundletype, vfs=vfs)

def overridechangegroupsubset(orig, repo, roots, heads, source, version = '01'):
    # we only care about performance for strips, not about 'hg bundle' and
    # similar
    if source != 'strip':
        return orig(repo, roots, heads, source, version=version)

    # below is all copied from changegroup.py, except with cg1 changed to
    # cg2
    cl = repo.changelog
    if not roots:
        roots = [nullid]
    # TODO: remove call to nodesbetween.
    csets, roots, heads = cl.nodesbetween(roots, heads)
    discbases = []
    for n in roots:
        discbases.extend([p for p in cl.parents(n) if p != nullid])
    outgoing = discovery.outgoing(cl, discbases, heads)
    # use packermap because other extensions might override it
    bundler = changegroup.packermap['02'][0](repo)
    gengroup = changegroup.getsubsetraw(repo, outgoing, bundler, source,
                                        fastpath=False)
    result = changegroup.cg2unpacker(util.chunkbuffer(gengroup), 'UN')
    result.version = '01' # needed to pass writebundle checks
    return result

def overridereadbundle(orig, ui, fh, fname, vfs=None):
    # copied from exchange.py
    header = changegroup.readexactly(fh, 4)

    alg = None
    if not fname:
        fname = "stream"
        if not header.startswith('HG') and header.startswith('\0'):
            fh = changegroup.headerlessfixup(fh, header)
            header = "HG10"
            alg = 'UN'
    elif vfs:
        fname = vfs.join(fname)

    magic, version = header[0:2], header[2:4]

    if magic != 'HG':
        raise util.Abort(_('%s: not a Mercurial bundle') % fname)
    if version == '10' or version == '2C':
        if alg is None:
            alg = changegroup.readexactly(fh, 2)
        if version == '10':
            return changegroup.cg1unpacker(fh, alg)
        else:
            return changegroup.cg2unpacker(fh, alg)
    elif version == '2Y':
        return bundle2.unbundle20(ui, fh, header=magic + version)
    else:
        raise util.Abort(_('%s: unknown bundle version %s') % (fname, version))

class cg2bundlerepository(bundlerepo.bundlerepository):
    def __init__(self, ui, path, bundlename):
        # copied from bundlerepo.py
        self._tempparent = None
        try:
            localrepo.localrepository.__init__(self, ui, path)
        except error.RepoError:
            self._tempparent = tempfile.mkdtemp()
            localrepo.instance(ui, self._tempparent, 1)
            localrepo.localrepository.__init__(self, ui, self._tempparent)
        self.ui.setconfig('phases', 'publish', False, 'bundlerepo')

        if path:
            self._url = 'bundle:' + util.expandpath(path) + '+' + bundlename
        else:
            self._url = 'bundle:' + bundlename

        self.tempfile = None
        f = util.posixfile(bundlename, "rb")
        self.bundle = exchange.readbundle(ui, f, bundlename)
        if self.bundle.compressed():
            fdtemp, temp = self.vfs.mkstemp(prefix="hg-bundle-",
                                            suffix=".hgun")
            self.tempfile = temp
            fptemp = os.fdopen(fdtemp, 'wb')

            try:
                if isinstance(self.bundle, changegroup.cg2unpacker):
                    header = "HG2CUN"
                else:
                    header = "HG10UN"
                fptemp.write(header)
                while True:
                    chunk = self.bundle.read(2**18)
                    if not chunk:
                        break
                    fptemp.write(chunk)
            finally:
                fptemp.close()

            f = self.vfs.open(self.tempfile, mode="rb")
            self.bundle = exchange.readbundle(ui, f, bundlename, self.vfs)

        # dict with the mapping 'filename' -> position in the bundle
        self.bundlefilespos = {}

        self.firstnewrev = self.changelog.repotiprev + 1
        phases.retractboundary(self, None, phases.draft,
                               [ctx.node() for ctx in self[self.firstnewrev:]])

bundlerepo.bundlerepository = cg2bundlerepository

def extsetup(ui):
    # add bundle types for changegroup2
    bundletypes = changegroup.bundletypes
    cg2types = {}
    for bundletype, hc in bundletypes.iteritems():
        if bundletype.startswith('HG10'):
            header, compressor = hc
            cg2type = 'HG2C' + bundletype[4:]
            cg2header = 'HG2C' + header[4:]
            cg2types[cg2type] = (cg2header, compressor)
    bundletypes.update(cg2types)

    extensions.wrapfunction(changegroup, 'writebundle', overridewritebundle)
    extensions.wrapfunction(changegroup, 'changegroupsubset',
                            overridechangegroupsubset)
    extensions.wrapfunction(exchange, 'readbundle', overridereadbundle)
