import os
from collections import defaultdict
from mercurial import mdiff, util
from mercurial.node import nullid, bin, hex
from mercurial.i18n import _
import shallowutil

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
            for node in nodes:
                ancestors.update(self.history.getancestors(filename, node))

            # Order the nodes children first, so we can produce reverse deltas
            orderednodes = reversed(self._toposort(ancestors))

            # getancestors() will return the ancestry of a commit, even across
            # renames. We currently don't support producing deltas across
            # renames, so we use dontprocess to store when an ancestory
            # traverses across a rename, so we can avoid processing those.
            dontprocess = set()

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

                # Record this child as the delta base for its parents.
                # This may be non optimal, since the parents may have many
                # children, and this will only choose the last one.
                # TODO: record all children and try all deltas to find best
                p1, p2, linknode, copyfrom = ancestors[node]

                if node in dontprocess:
                    if p1 != nullid:
                        dontprocess.add(p1)
                    if p2 != nullid:
                        dontprocess.add(p2)
                    continue

                if copyfrom:
                    dontprocess.add(p1)
                    p1 = nullid

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
        target.close()

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
                    p1 = nullid

                target.add(filename, node, p1, p2, linknode)

                if node in entries:
                    entries[node].historyrepacked = True

            count += 1
            ui.progress(_("repacking history"), count, unit="files",
                        total=len(byfile))

        ui.progress(_("repacking history"), None)
        target.close()

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

    def markdataentry(self, source, filename, node):
        """Mark the given filename+node revision as having a data rev in the
        given source.
        """
        entry = self._getorcreateentry(filename, node)
        entry.datasource = True
        self.sources.setdefault(source, set()).add(entry)

    def markhistoryentry(self, source, filename, node):
        """Mark the given filename+node revision as having a history rev in the
        given source.
        """
        entry = self._getorcreateentry(filename, node)
        entry.historysource = True
        self.sources.setdefault(source, set()).add(entry)

    def _getorcreateentry(self, filename, node):
        value = self.entries.get((filename, node))
        if not value:
            value = repackentry(filename, node)
            self.entries[(filename, node)] = value

        return value

class repackentry(object):
    """Simple class representing a single revision entry in the repackledger.
    """
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
