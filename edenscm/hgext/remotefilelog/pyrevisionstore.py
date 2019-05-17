# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial.rust.bindings import pyrevisionstore


datastore = pyrevisionstore.datastore
datapack = pyrevisionstore.datapack
historypack = pyrevisionstore.historypack
indexedlogdatastore = pyrevisionstore.indexedlogdatastore
repackdatapacks = pyrevisionstore.repackdatapacks
repackhistpacks = pyrevisionstore.repackhistpacks
repackincrementaldatapacks = pyrevisionstore.repackincrementaldatapacks
repackincrementalhistpacks = pyrevisionstore.repackincrementalhistpacks
