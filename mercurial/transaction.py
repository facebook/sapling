# transaction.py - simple journalling scheme for mercurial
#
# This transaction scheme is intended to gracefully handle program
# errors and interruptions. More serious failures like system crashes
# can be recovered with an fsck-like tool. As the whole repository is
# effectively log-structured, this should amount to simply truncating
# anything that isn't referenced in the changelog.
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import os

class transaction(object):
    def __init__(self, report, opener, journal, after=None, createmode=None):
        self.journal = None

        self.count = 1
        self.report = report
        self.opener = opener
        self.after = after
        self.entries = []
        self.map = {}
        self.journal = journal

        self.file = open(self.journal, "w")
        if createmode is not None:
            os.chmod(self.journal, createmode & 0666)

    def __del__(self):
        if self.journal:
            if self.entries: self.abort()
            self.file.close()
            try: os.unlink(self.journal)
            except: pass

    def add(self, file, offset, data=None):
        if file in self.map: return
        self.entries.append((file, offset, data))
        self.map[file] = len(self.entries) - 1
        # add enough data to the journal to do the truncate
        self.file.write("%s\0%d\n" % (file, offset))
        self.file.flush()

    def find(self, file):
        if file in self.map:
            return self.entries[self.map[file]]
        return None

    def replace(self, file, offset, data=None):
        if file not in self.map:
            raise KeyError(file)
        index = self.map[file]
        self.entries[index] = (file, offset, data)
        self.file.write("%s\0%d\n" % (file, offset))
        self.file.flush()

    def nest(self):
        self.count += 1
        return self

    def running(self):
        return self.count > 0

    def close(self):
        self.count -= 1
        if self.count != 0:
            return
        self.file.close()
        self.entries = []
        if self.after:
            self.after()
        else:
            os.unlink(self.journal)
        self.journal = None

    def abort(self):
        if not self.entries: return

        self.report(_("transaction abort!\n"))

        for f, o, ignore in self.entries:
            try:
                self.opener(f, "a").truncate(o)
            except:
                self.report(_("failed to truncate %s\n") % f)

        self.entries = []

        self.report(_("rollback completed\n"))

def rollback(opener, file):
    files = {}
    for l in open(file).readlines():
        f, o = l.split('\0')
        files[f] = o
    for f in files:
        o = files[f]
        opener(f, "a").truncate(int(o))
    os.unlink(file)

