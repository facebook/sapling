# remoterepo - remote repository proxy classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import repo

class remoterepository(repo.repository):
    pass

class remotelock(object):
    def __init__(self, repo):
        self.repo = repo
    def release(self):
        self.repo.unlock()
        self.repo = None
    def __del__(self):
        if self.repo:
            self.release()
