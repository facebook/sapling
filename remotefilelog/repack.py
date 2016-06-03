import os
from collections import defaultdict
from mercurial import error, mdiff, osutil, scmutil, util
from mercurial.node import nullid, bin, hex
from mercurial.i18n import _
import datapack, historypack, contentstore, metadatastore, shallowutil

def backgroundrepack(repo, incremental=True):
    cmd = util.hgcmd() + ['-R', repo.origroot, 'repack']
    incrementalstr = ''
    if incremental:
        cmd.append('--incremental')
        incrementalstr = 'incremental '
    cmd = ' '.join(map(util.shellquote, cmd))

    repo.ui.warn("(running background %srepack)\n" % incrementalstr)
    shallowutil.runshellcommand(cmd, os.environ)

def fullrepack(repo):
    datasource = contentstore.unioncontentstore(*repo.shareddatastores)
    historysource = metadatastore.unionmetadatastore(*repo.sharedhistorystores,
                                                     allowincomplete=True)

    _runrepack(repo, datasource, historysource)

def incrementalrepack(repo):
    """This repacks the repo by looking at the distribution of pack files in the
    repo and performing the most minimal repack to keep the repo in good shape.
    """

    packpath = shallowutil.getpackpath(repo)
    if not os.path.exists(packpath):
        os.makedirs(packpath)

    files = osutil.listdir(packpath, stat=True)

    datapacks = _computeincrementaldatapack(repo.ui, files)
    fullpaths = list(os.path.join(packpath, p) for p in datapacks)
    datapacks = list(datapack.datapack(p) for p in fullpaths)
    datapacks.extend(s for s in repo.shareddatastores
                     if not isinstance(s, datapack.datapackstore))

    historypacks = _computeincrementalhistorypack(repo.ui, files)
    fullpaths = list(os.path.join(packpath, p) for p in historypacks)
    historypacks = list(historypack.historypack(p) for p in fullpaths)
    historypacks.extend(s for s in repo.sharedhistorystores
                        if not isinstance(s, historypack.historypackstore))

    datasource = contentstore.unioncontentstore(*datapacks)
    historysource = metadatastore.unionmetadatastore(*historypacks,
                                                     allowincomplete=True)

    _runrepack(repo, datasource, historysource)

def _computeincrementaldatapack(ui, files):
    """Given a set of pack files and a set of generation size limits, this
    function computes the list of files that should be packed as part of an
    incremental repack.

    It tries to strike a balance between keeping incremental repacks cheap (i.e.
    packing small things when possible, and rolling the packs up to the big ones
    over time).
    """
    generations = ui.configlist("remotefilelog", "data.generations",
                                ['1GB', '100MB', '1MB'])
    generations = list(sorted((util.sizetoint(s) for s in generations),
                                reverse=True))
    generations.append(0)

    gencountlimit = ui.configint('remotefilelog', 'data.gencountlimit', 2)
    repacksizelimit = ui.configbytes('remotefilelog', 'data.repacksizelimit',
                                     '100MB')

    return _computeincrementalpack(ui, files, generations, datapack.PACKSUFFIX,
            datapack.INDEXSUFFIX, gencountlimit, repacksizelimit)

def _computeincrementalhistorypack(ui, files):
    generations = ui.configlist("remotefilelog", "history.generations",
                                ['100MB'])
    generations = list(sorted((util.sizetoint(s) for s in generations),
                                reverse=True))
    generations.append(0)

    gencountlimit = ui.configint('remotefilelog', 'history.gencountlimit', 2)
    repacksizelimit = ui.configbytes('remotefilelog', 'history.repacksizelimit',
                                     '100MB')

    return _computeincrementalpack(ui, files, generations,
            historypack.PACKSUFFIX, historypack.INDEXSUFFIX, gencountlimit,
            repacksizelimit)

def _computeincrementalpack(ui, files, limits, packsuffix, indexsuffix,
                            gencountlimit, repacksizelimit):
    # Group the packs by generation (i.e. by size)
    generations = []
    for i in xrange(len(limits)):
        generations.append([])
    sizes = {}
    fileset = set(fn for fn, mode, stat in files)
    for filename, mode, stat in files:
        if not filename.endswith(packsuffix):
            continue

        prefix = filename[:-len(packsuffix)]

        # Don't process a pack if it doesn't have an index.
        if (prefix + indexsuffix) not in fileset:
            continue

        size = stat.st_size
        sizes[prefix] = size
        for i, limit in enumerate(limits):
            if size > limit:
                generations[i].append(prefix)
                break

    # Find the largest generation with more than 2 packs and repack it.
    for i, limit in enumerate(limits):
        if len(generations[i]) > gencountlimit:
            # Generally we only want to repack 2 things at once, but if the
            # whole generation is small, let's just do it all!
            count = 2
            if sum(sizes[n] for n in generations[i]) < repacksizelimit:
                count = len(generations[i])
            return sorted(generations[i], key=lambda x: sizes[x])[:count]

    # If no generation has more than 2 packs, repack as many as fit into the
    # limit
    small = set().union(*generations[1:])
    if len(small) > 1:
        total = 0
        packs = []
        for pack in sorted(small, key=lambda x: sizes[x]):
            size = sizes[pack]
            if total + size < repacksizelimit:
                packs.append(pack)
                total += size
            else:
                break

        if len(packs) > 1:
            return packs

    # If there aren't small ones to repack, repack the two largest ones.
    if len(generations[0]) > 1:
        return generations[0]

    return []

def _runrepack(repo, data, history):
    packpath = shallowutil.getpackpath(repo)
    util.makedirs(packpath)

    packer = repacker(repo, data, history)

    opener = scmutil.vfs(packpath)
    # Packs should be write-once files, so set them to read-only.
    opener.createmode = 0o444
    with datapack.mutabledatapack(opener) as dpack:
        with historypack.mutablehistorypack(opener) as hpack:
            try:
                packer.run(dpack, hpack)
            except error.LockHeld:
                raise error.Abort(_("skipping repack - another repack is "
                                    "already running"))

class repacker(object):
    """Class for orchestrating the repack of data and history information into a
    new format.
    """
    def __init__(self, repo, data, history):
        self.repo = repo
        self.data = data
        self.history = history

    def run(self, targetdata, targethistory):
        ledger = repackledger()

        with self.repo._lock(self.repo.svfs, "repacklock", False, None,
                             None, _('repacking %s') % self.repo.origroot):
            self.repo.hook('prerepack')
            # Populate ledger from source
            self.data.markledger(ledger)
            self.history.markledger(ledger)

            # Run repack
            self.repackdata(ledger, targetdata)
            self.repackhistory(ledger, targethistory)

            # Call cleanup on each source
            for source in ledger.sources:
                source.cleanup(ledger)

    def repackdata(self, ledger, target):
        ui = self.repo.ui

        byfile = {}
        for entry in ledger.entries.itervalues():
            if entry.datasource:
                byfile.setdefault(entry.filename, {})[entry.node] = entry

        count = 0
        for filename, entries in sorted(byfile.iteritems()):
            ancestors = {}
            nodes = list(node for node in entries.iterkeys())
            nohistory = []
            for node in nodes:
                if node in ancestors:
                    continue
                try:
                    ancestors.update(self.history.getancestors(filename, node))
                except KeyError:
                    # Since we're packing data entries, we may not have the
                    # corresponding history entries for them. It's not a big
                    # deal, but the entries won't be delta'd perfectly.
                    nohistory.append(node)

            # Order the nodes children first, so we can produce reverse deltas
            orderednodes = list(reversed(self._toposort(ancestors)))
            orderednodes.extend(sorted(nohistory))

            # Compute deltas and write to the pack
            deltabases = defaultdict(lambda: nullid)
            nodes = set(nodes)
            for node in orderednodes:
                # orderednodes is all ancestors, but we only want to serialize
                # the files we have.
                if node not in nodes:
                    continue

                # Find delta base
                # TODO: allow delta'ing against most recent descendant instead
                # of immediate child
                deltabase = deltabases[node]

                # Use available ancestor information to inform our delta choices
                ancestorinfo = ancestors.get(node)
                if ancestorinfo:
                    p1, p2, linknode, copyfrom = ancestorinfo

                    # The presence of copyfrom means we're at a point where the
                    # file was copied from elsewhere. So don't attempt to do any
                    # deltas with the other file.
                    if copyfrom:
                        p1 = nullid

                    # Record this child as the delta base for its parents.
                    # This may be non optimal, since the parents may have many
                    # children, and this will only choose the last one.
                    # TODO: record all children and try all deltas to find best
                    if p1 != nullid:
                        deltabases[p1] = node
                    if p2 != nullid:
                        deltabases[p2] = node

                # Compute delta
                # TODO: reuse existing deltas if it matches our deltabase
                if deltabase != nullid:
                    deltabasetext = self.data.get(filename, deltabase)
                    original = self.data.get(filename, node)
                    delta = mdiff.textdiff(deltabasetext, original)
                else:
                    delta = self.data.get(filename, node)

                # TODO: don't use the delta if it's larger than the fulltext
                target.add(filename, node, deltabase, delta)

                entries[node].datarepacked = True

            count += 1
            ui.progress(_("repacking data"), count, unit="files",
                        total=len(byfile))

        ui.progress(_("repacking data"), None)
        target.close(ledger=ledger)

    def repackhistory(self, ledger, target):
        ui = self.repo.ui

        byfile = {}
        for entry in ledger.entries.itervalues():
            if entry.historysource:
                byfile.setdefault(entry.filename, {})[entry.node] = entry

        count = 0
        for filename, entries in sorted(byfile.iteritems()):
            ancestors = {}
            nodes = list(node for node in entries.iterkeys())

            for node in nodes:
                if node in ancestors:
                    continue
                ancestors.update(self.history.getancestors(filename, node))

            # Order the nodes children first
            orderednodes = reversed(self._toposort(ancestors))

            # Write to the pack
            dontprocess = set()
            for node in orderednodes:
                p1, p2, linknode, copyfrom = ancestors[node]

                if node in dontprocess:
                    if p1 != nullid:
                        dontprocess.add(p1)
                    if p2 != nullid:
                        dontprocess.add(p2)
                    continue

                if copyfrom:
                    dontprocess.add(p1)

                target.add(filename, node, p1, p2, linknode, copyfrom)

                if node in entries:
                    entries[node].historyrepacked = True

            count += 1
            ui.progress(_("repacking history"), count, unit="files",
                        total=len(byfile))

        ui.progress(_("repacking history"), None)
        target.close(ledger=ledger)

    def _toposort(self, ancestors):
        def parentfunc(node):
            p1, p2, linknode, copyfrom = ancestors[node]
            parents = []
            if p1 != nullid:
                parents.append(p1)
            if p2 != nullid:
                parents.append(p2)
            return parents

        sortednodes = shallowutil.sortnodes(ancestors.keys(), parentfunc)
        return sortednodes

class repackledger(object):
    """Storage for all the bookkeeping that happens during a repack. It contains
    the list of revisions being repacked, what happened to each revision, and
    which source store contained which revision originally (for later cleanup).
    """
    def __init__(self):
        self.entries = {}
        self.sources = {}
        self.created = set()

    def markdataentry(self, source, filename, node):
        """Mark the given filename+node revision as having a data rev in the
        given source.
        """
        entry = self._getorcreateentry(filename, node)
        entry.datasource = True
        entries = self.sources.get(source)
        if not entries:
            entries = set()
            self.sources[source] = entries
        entries.add(entry)

    def markhistoryentry(self, source, filename, node):
        """Mark the given filename+node revision as having a history rev in the
        given source.
        """
        entry = self._getorcreateentry(filename, node)
        entry.historysource = True
        entries = self.sources.get(source)
        if not entries:
            entries = set()
            self.sources[source] = entries
        entries.add(entry)

    def _getorcreateentry(self, filename, node):
        key = (filename, node)
        value = self.entries.get(key)
        if not value:
            value = repackentry(filename, node)
            self.entries[key] = value

        return value

    def addcreated(self, value):
        self.created.add(value)

class repackentry(object):
    """Simple class representing a single revision entry in the repackledger.
    """
    __slots__ = ['filename', 'node', 'datasource', 'historysource',
                 'datarepacked', 'historyrepacked']
    def __init__(self, filename, node):
        self.filename = filename
        self.node = node
        # If the revision has a data entry in the source
        self.datasource = False
        # If the revision has a history entry in the source
        self.historysource = False
        # If the revision's data entry was repacked into the repack target
        self.datarepacked = False
        # If the revision's history entry was repacked into the repack target
        self.historyrepacked = False
