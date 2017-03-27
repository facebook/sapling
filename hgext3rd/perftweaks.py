# perftweaks.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""extension for tweaking Mercurial features to improve performance.

::

    [perftweaks]
    # Whether to use faster hidden cache. It has faster cache hash calculation
    # which only check stat of a few files inside store/ directory.
    fasthiddencache = False
"""

from mercurial import (
    branchmap,
    dispatch,
    error,
    extensions,
    merge,
    phases,
    repoview,
    revlog,
    scmutil,
    tags,
    util,
)
from mercurial.extensions import wrapfunction
from mercurial.node import bin, nullid, nullrev
import errno
import hashlib
import os

cachefile = repoview.cachefile
testedwith = 'ships-with-fb-hgext'

def extsetup(ui):
    wrapfunction(tags, '_readtagcache', _readtagcache)
    wrapfunction(merge, '_checkcollision', _checkcollision)
    wrapfunction(branchmap.branchcache, 'update', _branchmapupdate)
    if ui.configbool('perftweaks', 'preferdeltas'):
        wrapfunction(revlog.revlog, '_isgooddelta', _isgooddelta)

    wrapfunction(dispatch, 'runcommand', _trackdirstatesizes)
    wrapfunction(dispatch, 'runcommand', _tracksparseprofiles)

    # noderev cache creation
    # The node rev cache is a cache of rev numbers that we are likely to do a
    # node->rev lookup for. Since looking up rev->node is cheaper than
    # node->rev, we use this cache to prefill the changelog radix tree with
    # mappings.
    wrapfunction(branchmap.branchcache, 'write', _branchmapwrite)
    wrapfunction(phases.phasecache, 'advanceboundary', _editphases)
    wrapfunction(phases.phasecache, 'retractboundary', _editphases)
    try:
        remotenames = extensions.find('remotenames')
        wrapfunction(remotenames, 'saveremotenames', _saveremotenames)
    except KeyError:
        pass
    if ui.config('perftweaks', 'fasthiddencache'):
        repoview.filtertable['visible'] = _computehidden

def reposetup(ui, repo):
    if repo.local() is not None:
        _preloadrevs(repo)

def _readtagcache(orig, ui, repo):
    """Disables reading tags if the repo is known to not contain any."""
    if ui.configbool('perftweaks', 'disabletags'):
        return (None, None, None, {}, False)

    return orig(ui, repo)

def _checkcollision(orig, repo, wmf, actions):
    """Disables case collision checking since it is known to be very slow."""
    if repo.ui.configbool('perftweaks', 'disablecasecheck'):
        return
    orig(repo, wmf, actions)

def _branchmapupdate(orig, self, repo, revgen):
    if not repo.ui.configbool('perftweaks', 'disablebranchcache'):
        return orig(self, repo, revgen)

    cl = repo.changelog

    # Since we have no branches, the default branch heads are equal to
    # cl.headrevs().
    branchheads = sorted(cl.headrevs())

    self['default'] = [cl.node(rev) for rev in branchheads]
    tiprev = branchheads[-1]
    if tiprev > self.tiprev:
        self.tipnode = cl.node(tiprev)
        self.tiprev = tiprev

    # Copy and paste from branchmap.branchcache.update()
    if not self.validfor(repo):
        # cache key are not valid anymore
        self.tipnode = nullid
        self.tiprev = nullrev
        for heads in self.values():
            tiprev = max(cl.rev(node) for node in heads)
            if tiprev > self.tiprev:
                self.tipnode = cl.node(tiprev)
                self.tiprev = tiprev
    self.filteredhash = scmutil.filteredhash(repo, self.tiprev)
    repo.ui.log('branchcache', 'perftweaks updated %s branch cache\n',
                repo.filtername)

def _branchmapwrite(orig, self, repo):
    result = orig(self, repo)
    if repo.ui.configbool('perftweaks', 'cachenoderevs', True):
        revs = set()
        nodemap = repo.changelog.nodemap
        for branch, heads in self.iteritems():
            revs.update(nodemap[n] for n in heads)
        name = 'branchheads-%s' % repo.filtername
        _savepreloadrevs(repo, name, revs)

    return result

def _saveremotenames(orig, repo, remotepath, branches=None, bookmarks=None):
    result = orig(repo, remotepath, branches=branches, bookmarks=bookmarks)
    if repo.ui.configbool('perftweaks', 'cachenoderevs', True):
        revs = set()
        nodemap = repo.changelog.nodemap
        if bookmarks:
            for b, n in bookmarks.iteritems():
                n = bin(n)
                # remotenames can pass bookmarks that don't exist in the
                # changelog yet. It filters them internally, but we need to as
                # well.
                if n in nodemap:
                    revs.add(nodemap[n])
        if branches:
            for branch, nodes in branches.iteritems():
                for n in nodes:
                    if n in nodemap:
                        revs.add(nodemap[n])

        name = 'remotenames-%s' % remotepath
        _savepreloadrevs(repo, name, revs)

    return result

def _editphases(orig, self, repo, tr, *args):
    result = orig(self, repo, tr, *args)
    def _write(fp):
        revs = set()
        nodemap = repo.changelog.nodemap
        for phase, roots in enumerate(self.phaseroots):
            for n in roots:
                if n in nodemap:
                    revs.add(nodemap[n])
        _savepreloadrevs(repo, 'phaseroots', revs)

    # We don't actually use the transaction file generator. It's just a hook so
    # we can write out at the same time as phases.
    if tr:
        tr.addfilegenerator('noderev-phaseroot', ('phaseroots-fake',), _write)
    else:
        # fp is not used anyway
        _write(fp=None)

    return result

def _isgooddelta(orig, self, d, textlen):
    """Returns True if the given delta is good. Good means that it is within
    the disk span, disk size, and chain length bounds that we know to be
    performant."""
    if d is None:
        return False

    # - 'dist' is the distance from the base revision -- bounding it limits
    #   the amount of I/O we need to do.
    # - 'compresseddeltalen' is the sum of the total size of deltas we need
    #   to apply -- bounding it limits the amount of CPU we consume.
    dist, l, data, base, chainbase, chainlen, compresseddeltalen = d

    # Our criteria:
    # 1. the delta is not larger than the full text
    # 2. the delta chain cumulative size is not greater than twice the fulltext
    # 3. The chain length is less than the maximum
    #
    # This differs from upstream Mercurial's criteria. They prevent the total
    # ondisk span from chain base to rev from being greater than 4x the full
    # text len. This isn't good enough in our world since if we have 10+
    # branches going on at once, we can easily exceed the 4x limit and cause
    # full texts to be written over and over again.
    if (l > textlen or compresseddeltalen > textlen * 2 or
        (self._maxchainlen and chainlen > self._maxchainlen)):
        return False

    return True

def _cachefilename(name):
    return 'cache/noderevs/%s' % name

def _preloadrevs(repo):
    # Preloading the node-rev map for likely to be used revs saves 100ms on
    # every command. This is because normally to look up a node, hg has to scan
    # the changelog.i file backwards, potentially reading through hundreds of
    # thousands of entries and building a cache of them.  Looking up a rev
    # however is fast, because we know exactly what offset in the file to read.
    # Reading old commits is common, since the branchmap needs to to convert old
    # branch heads from node to rev.

    if repo.ui.configbool('perftweaks', 'cachenoderevs', True):
        repo = repo.unfiltered()
        revs = set()
        cachedir = repo.vfs.join('cache', 'noderevs')
        try:
            for cachefile in os.listdir(cachedir):
                filename = _cachefilename(cachefile)
                revs.update(int(r) for r in repo.vfs.open(filename).readlines())

            getnode = repo.changelog.node
            nodemap = repo.changelog.nodemap
            for r in revs:
                try:
                    node = getnode(r)
                    nodemap[node] = r
                except (IndexError, ValueError):
                    # Rev no longer exists or rev is out of range
                    pass
        except EnvironmentError:
            # No permission to read? No big deal
            pass

def _savepreloadrevs(repo, name, revs):
    if repo.ui.configbool('perftweaks', 'cachenoderevs', True):
        cachedir = repo.vfs.join('cache', 'noderevs')
        try:
            repo.vfs.mkdir(cachedir)
        except OSError as ex:
            # If we failed because the directory already exists,
            # continue.  In all other cases (e.g., no permission to create the
            # directory), just silently return without doing anything.
            if ex.errno != errno.EEXIST:
                return

        try:
            filename = _cachefilename(name)
            f = repo.vfs.open(filename, mode='w+', atomictemp=True)
            f.write('\n'.join(str(r) for r in revs))
            f.close()
        except EnvironmentError:
            # No permission to write? No big deal
            pass

def _trackdirstatesizes(runcommand, lui, repo, *args):
    res = runcommand(lui, repo, *args)
    if repo is not None and repo.local():
        dirstate = repo.dirstate
        # if the _map attribute is missing on the map, the dirstate was not
        # loaded. If present, it *could* be a sqldirstate map.
        if '_map' in vars(dirstate):
            map_ = dirstate._map
            # check for a sqldirstate, only load length if cache was activated
            sqldirstate = util.safehasattr(map_, '_lookupcache')
            logsize = map_._lookupcache is not None if sqldirstate else True
            if logsize:
                lui.log('dirstate_size', '', dirstate_size=len(dirstate._map))
    return res

def _tracksparseprofiles(runcommand, lui, repo, *args):
    res = runcommand(lui, repo, *args)
    if repo is not None and repo.local():
        if util.safehasattr(repo, 'getactiveprofiles'):
            profiles = repo.getactiveprofiles()
            lui.log('sparse_profiles', '',
                    active_profiles=','.join(sorted(profiles)))
    return res

def _computehidden(repo):
    """compute the set of hidden revision to filter

    During most operation hidden should be filtered.
    This function is similar to upstream function repoview.computehidden()
    Main difference is in reading and writing hidden cache.
    For example, it uses another _cachehash() function."""
    assert not repo.changelog.filteredrevs

    if not repo.obsstore:
        return frozenset()
    cachekey = _cachehash(repo)
    hidden = repoview._readhiddencache(repo, cachefile, cachekey)
    isbundlerepo = (util.safehasattr(repo, '_url') and
                    repo._url.startswith('bundle'))
    # bundlerepo may add new nodes that are already obsoleted but the cache
    # key is the same. To avoid it let's not use cache for bundlerepo
    if hidden is None or isbundlerepo:
        repo.ui.debug('recomputing hidden cache\n')
        hidden = frozenset(repoview._getstatichidden(repo))
        if not isbundlerepo:
            _trywritehiddencache(repo, cachekey, hidden)
    else:
        repo.ui.debug('using hidden cache\n')

    # check if we have wd parents, bookmarks or tags pointing to hidden
    # changesets and remove those.
    # This part of code is almost identical to core mercurial
    # repoview.computehidden().
    dynamic = hidden & repoview._getdynamicblockers(repo)
    if dynamic:
        cl = repo.changelog
        blocked = cl.ancestors(dynamic, inclusive=True)
        hidden = frozenset(r for r in hidden if r not in blocked)
    return hidden

def _cachehash(repo):
    h = hashlib.sha1()
    for filename in ['phaseroots', '00changelog.i', 'obsstore', 'obsinhibit']:
        size, mtime = _safegetsizemtime(repo, filename)
        h.update(str(size))
        h.update(str(mtime))
    # changelog may delay update to the 00changelog.i file. In this case
    # files' size + mtime is the same, but static hidden cache is different.
    # Let's add delayed value to the cache key
    h.update(str(repo.changelog._delayed))
    return h.digest()

def _safegetsizemtime(repo, filename):
    if repo.svfs.exists(filename):
        stat = repo.svfs.stat(filename)
        return stat.st_size, stat.st_mtime
    else:
        return 0, 0

def _trywritehiddencache(repo, newhash, hidden):
    """write cache of hidden changesets to disk

    Will not write the cache if a wlock cannot be obtained lazily.
    The cache consists of a head of 22byte:
       2 byte    version number of the cache
      20 byte    sha1 to validate the cache
     n*4 byte    hidden revs
    """
    wlock = fh = None
    try:
        wlock = repo.wlock(wait=False)
        # write cache to file
        fh = repo.vfs.open(cachefile, 'w+b', atomictemp=True)
        repoview._writehiddencache(fh, newhash, hidden)
        fh.close()
    except (IOError, OSError):
        repo.ui.debug('error writing hidden changesets cache\n')
    except error.LockHeld:
        repo.ui.debug('cannot obtain lock to write hidden changesets cache\n')
    finally:
        if wlock:
            wlock.release()
