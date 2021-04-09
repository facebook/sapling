# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from bindings import blackbox as _blackbox


events = _blackbox.events
init = _blackbox.init
log = _blackbox.log
sessions = _blackbox.sessions
sync = _blackbox.sync


def shortlist(listlike, count=None, limit=4):
    """Return a value that can be converted to Rust blackbox::event::ShortList"""
    shortlist = []
    for name in listlike:
        shortlist.append(name)
        if len(shortlist) > limit:
            break
    if count is None:
        count = len(listlike)
    return {"short_list": shortlist, "len": count}


class logblocked(object):
    def __new__(cls, op, seconds=None, name=None, ignorefast=False):
        """Log a "Blocked" event.

        If seconds is None, then this should be used as a context manager,
        and seconds will be calculated automatically. Otherwise, this
        function will log a "Blocked" event immediately.

        If name is not None, then additional name will be logged. This is
        useful for things like hook name where "op" is "hook", "name" is
        the actual hook name.

        If ignorefast is True, then a fast operation will be ignored.
        """
        if seconds is not None:
            # Non-context manager version
            millis = int(seconds * 1000)
            if not ignorefast or millis >= 10:
                log({"blocked": {"op": op, "duration_ms": millis, "name": name}})
            return None
        else:
            self = super(logblocked, cls).__new__(cls)
            self.op = op
            self.name = name
            self.ignorefast = ignorefast
            return self

    def __enter__(self):
        self.starttime = _timer()

    def __exit__(self, exctype, excval, exctb):
        seconds = _timer() - self.starttime
        millis = int(seconds * 1000)
        if not self.ignorefast or millis >= 10:
            log({"blocked": {"op": self.op, "duration_ms": millis, "name": self.name}})


def _timer():
    """util.timer"""
    from . import util  # avoid cycles

    globals()["_timer"] = util.timer  # replace self
    return util.timer()
