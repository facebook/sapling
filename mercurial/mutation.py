# mutation.py - commit mutation tracking
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import node as nodemod, util


def record(repo, extra, prednodes, op=None):
    for key in "mutpred", "mutuser", "mutdate", "mutop":
        if key in extra:
            del extra[key]
    if recording(repo):
        extra["mutpred"] = ",".join(nodemod.hex(p) for p in prednodes)
        extra["mutuser"] = repo.ui.config("mutation", "user") or repo.ui.username()
        date = repo.ui.config("mutation", "date")
        if date is None:
            date = util.makedate()
        else:
            date = util.parsedate(date)
        extra["mutdate"] = "%d %d" % date
        if op is not None:
            extra["mutop"] = op


def recording(repo):
    return repo.ui.configbool("mutation", "record")


def enabled(repo):
    return repo.ui.configbool("mutation", "enabled")
