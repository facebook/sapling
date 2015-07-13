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
    if source != 'strip' or version != '01':
        return orig(repo, roots, heads, source, version=version)

    # below is all copied from changegroup.py, except with cg1 changed to
    # cg2
    cl = repo.changelog
    if not roots:
        roots = [nullid]
    discbases = []
    for n in roots:
        discbases.extend([p for p in cl.parents(n) if p != nullid])
    # TODO: remove call to nodesbetween.
    csets, roots, heads = cl.nodesbetween(roots, heads)
    included = set(csets)
    discbases = [n for n in discbases if n not in included]
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
    elif version == '20':
        return bundle2.unbundle20(ui, fh)
    else:
        raise util.Abort(_('%s: unknown bundle version %s') % (fname, version))

class cg2bundlerepository(bundlerepo.bundlerepository):
    def __init__(self, ui, path, bundlename):
        self.cg2temp = None
        f = util.posixfile(bundlename, "rb")
        bundle = exchange.readbundle(ui, f, bundlename)
        if bundle.compressed and isinstance(bundle, changegroup.cg2unpacker):
            fdtemp, bundlename = tempfile.mkstemp(prefix="hg-bundle-",
                                            suffix=".hgun")
            self.cg2temp = bundlename
            fptemp = os.fdopen(fdtemp, 'wb')

            try:
                fptemp.write("HG2CUN")
                while True:
                    chunk = bundle.read(2**18)
                    if not chunk:
                        break
                    fptemp.write(chunk)
            finally:
                fptemp.close()
            pass
        f.close()
        super(cg2bundlerepository, self).__init__(ui, path, bundlename)

    def close(self):
        super(cg2bundlerepository, self).close()
        if self.cg2temp:
            os.unlink(self.cg2temp)

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
