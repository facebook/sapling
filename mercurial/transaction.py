# transaction.py - simple journalling scheme for mercurial
#
# This transaction scheme is intended to gracefully handle program
# errors and interruptions. More serious failures like system crashes
# can be recovered with an fsck-like tool. As the whole repository is
# effectively log-structured, this should amount to simply truncating
# anything that isn't referenced in the changelog.
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os
from i18n import gettext as _

class transaction:
    def __init__(self, report, opener, journal, after=None):
        self.journal = None

        # abort here if the journal already exists
        if os.path.exists(journal):
            raise AssertionError(_("journal already exists - run hg recover"))

        self.report = report
        self.opener = opener
        self.after = after
        self.entries = []
        self.map = {}
        self.journal = journal

        self.file = open(self.journal, "w")

    def __del__(self):
        if self.journal:
            if self.entries: self.abort()
            self.file.close()
            try: os.unlink(self.journal)
            except: pass

    def add(self, file, offset):
        if file in self.map: return
        self.entries.append((file, offset))
        self.map[file] = 1
        # add enough data to the journal to do the truncate
        self.file.write("%s\0%d\n" % (file, offset))
        self.file.flush()

    def close(self):
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

        for f, o in self.entries:
            try:
                self.opener(f, "a").truncate(o)
            except:
                self.report(_("failed to truncate %s\n") % f)

        self.entries = []

        self.report(_("rollback completed\n"))

def rollback(opener, file):
    for l in open(file).readlines():
        f, o = l.split('\0')
        opener(f, "a").truncate(int(o))
    os.unlink(file)

