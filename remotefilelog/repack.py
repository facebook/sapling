import os
from collections import defaultdict
from mercurial import util
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
        pass

    def repackhistory(self, ledger, target):
        pass

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
