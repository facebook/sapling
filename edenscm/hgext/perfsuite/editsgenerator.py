# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import itertools
import random

from edenscm.mercurial import context


# TODO: make these configs
MAX_FILES_PER_COMMIT = 10000
DESIRED_FILES_PER_COMMIT = 6
MAX_EDITS_PER_FILE = 6
FILE_DELETION_CHANCE = 6  # percent
ADD_DELETE_RATIO = 3
DELETION_MAX_SIZE = 2000

BLACKLIST = [".hgdirsync", ".hgtags"]


class randomeditsgenerator(object):
    def __init__(self, ctx):
        """``ctx`` is used to build a set of paths"""
        self.ui = ctx.repo().ui
        self.pathchars = "abcdefghijklmnopqrstuvwxyz"
        self.dirs = self.makedirs()
        self.fnames = self.makefilenames()

    def makedirs(self):
        dircount = self.ui.configint("repogenerator", "filenamedircount", 3)

        # We generate all path combinations up-front and then shuffle them to
        # try and distribute the paths. (`` getrandompath`` only selects from
        # a prorated slice of this list based on generation progress, so if it
        # were not randomized, initial edis would all be in a/a/*.)
        #
        # One downside to this approach: third-level directories are still too
        # sparse initially.
        paths = list(itertools.product(self.pathchars, repeat=dircount))
        random.shuffle(paths)
        return paths

    def makefilenames(self):
        leaflen = self.ui.configint("repogenerator", "filenameleaflength", 3)

        # Unlike with dirs, there's no upside to randomizing, so keep
        # alphabetical for simplicity.
        return list(itertools.product(self.pathchars, repeat=leaflen))

    def getrandompath(self, wctx):
        # Limit the dictionary of directory names initially, and expand them
        # over time (based on our goal) to mimic the organic growth of
        # directories and projects.
        maxdir = max(1, int(len(self.dirs) * self.getcompletionratio(wctx)))
        dirparts = random.sample(self.dirs[0:maxdir], 1)[0]

        # Same thing but for filenames.
        maxfname = max(1, int(len(self.fnames) * self.getcompletionratio(wctx)))
        fnparts = random.sample(self.fnames[0:maxfname], 1)[0]

        return "/".join(dirparts) + "/" + "".join(fnparts)

    def getcompletionratio(self, wctx):
        tiprev = wctx.p1().rev() + 1  # rev is 0-based
        goalrev = self.ui.configint("repogenerator", "numcommits", 10000)
        return float(tiprev) / goalrev

    def makerandomedits(self, wctx):
        i = 0
        while i < DESIRED_FILES_PER_COMMIT:
            path = self.getrandompath(wctx)
            existingdata = ""
            if isinstance(wctx, context.workingctx):
                path = path.encode("ascii", "ignore")

            if wctx[path].exists():
                if random.randrange(0, 100) <= FILE_DELETION_CHANCE:
                    wctx[path].remove()
                else:
                    existingdata = wctx[path].data()
            else:
                existingdata = "new file\n"

            if len(existingdata) > 0:
                for _ in range(0, random.randrange(1, MAX_EDITS_PER_FILE)):
                    if len(existingdata) <= 1:
                        existingdata = "new file"
                        break
                    if random.randrange(0, 10) > ADD_DELETE_RATIO:
                        idx = random.randrange(0, len(existingdata))
                        existingdata = (
                            existingdata[:idx]
                            + "/* random data: %s */\n" % random.randrange(0, 900000)
                            + existingdata[idx:]
                        )
                    else:
                        length = random.randrange(
                            0, min(len(existingdata) - 1, DELETION_MAX_SIZE)
                        )
                        idx = random.randrange(0, len(existingdata) - length)
                        existingdata = (
                            existingdata[:idx] + existingdata[(idx + length) :]
                        )

                wctx[path].write(existingdata, "")
            i += 1
