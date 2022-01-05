# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# replay.py - types and utils for unbundle replays.

from __future__ import absolute_import

from . import json


class ReplayData(object):
    """A structure to store and serialize/deserialize replay-related data

    Replay is a process of re-application of an `unbundle`, captured on
    the wire in some other system. Such re-application might have some
    expectations, which we need to be able to verify, and some additional
    data, which we may need to use. Currently, the following are stored:
      `commitdates` - a map form the original hash to the commit date to
                      be used in a rebased commit. Must be in the format,
                      understood by `util.parsedate`.
     - `ontobook` - a bookmark, used for pushrebase
     - `rebasedhead`- an expected hash of the rebased head
     - `hgbonsaimapping` - mapping from hg changesets to bonsai changesets.
                           Normally it's used only by mononoke
    """

    def __init__(self, commitdates, rebasedhead, ontobook, hgbonsaimapping):
        self.commitdates = commitdates
        self.rebasedhead = rebasedhead
        self.ontobook = ontobook
        self.hgbonsaimapping = hgbonsaimapping

    def serialize(self):
        res = {
            "commitdates": self.commitdates,
            "rebasedhead": self.rebasedhead,
            "ontobook": self.ontobook,
            "hgbonsaimapping": self.hgbonsaimapping,
        }
        return json.dumps(res)

    @classmethod
    def deserialize(cls, s):
        d = json.loads(s)
        commitdates = d.get("commitdates", {})
        rebasedhead = d.get("rebasedhead")
        ontobook = d.get("ontobook")
        hgbonsaimapping = d.get("hgbonsaimapping", {})
        return cls(commitdates, rebasedhead, ontobook, hgbonsaimapping)

    def getcommitdate(self, ui, commithash, commitdate):
        saveddate = self.commitdates.get(commithash)
        if saveddate:
            return (int(saveddate), commitdate[1])
        return commitdate
