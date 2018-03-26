# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import random

from mercurial import (
    context,
)

# TODO: make these configs
MAX_FILES_PER_COMMIT = 10000
DESIRED_FILES_PER_COMMIT = 6
MAX_EDITS_PER_FILE = 6
FILE_DELETION_CHANCE = 6 # percent
ADD_DELETE_RATIO = 3
DELETION_MAX_SIZE = 2000

BLACKLIST = (['.hgdirsync', '.hgtags'])

class randomeditsgenerator(object):
    def __init__(self, ctx):
        """``ctx`` is used to build a set of paths"""
        self.paths = None
        self.usepathsfrom(ctx)

    def usepathsfrom(self, ctx):
        self.paths = []
        for path in ctx:
            self.paths.append(path)

    def getrandompath(self):
        while True:
            path = self.paths[random.randrange(0, len(self.paths))]
            if path not in BLACKLIST:
                return path

    def makerandomedits(self, wctx):
        i = 0
        while i < DESIRED_FILES_PER_COMMIT:
            path = self.getrandompath()
            existingdata = ''
            if isinstance(wctx, context.workingctx):
                path = path.encode('ascii', 'ignore')

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
                        existingdata = existingdata[:idx] + \
                                       "/* random data: %s */\n" % \
                                       random.randrange(0, 900000) + \
                                       existingdata[idx:]
                    else:
                        length = random.randrange(0,
                            min(len(existingdata) - 1, DELETION_MAX_SIZE))
                        idx = random.randrange(0, len(existingdata) - length)
                        existingdata = existingdata[:idx] + \
                                       existingdata[(idx + length):]

                wctx[path].write(existingdata, '')
            i += 1
