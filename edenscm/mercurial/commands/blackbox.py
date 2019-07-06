# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import time

from ..blackbox import filter
from ..i18n import _
from .cmdtable import command


@command(
    "blackbox",
    [
        ("s", "start", 15, _("start time (minutes in the past, relative to now)")),
        ("e", "end", 0, _("end time (minutes in the past, relative to now)")),
        ("p", "pattern", "", _("JSON pattern to match (ADVANCED)")),
        ("", "timestamp", True, _("show timestamp (ADVANCED)")),
        ("", "sid", True, _("show session id (ADVANCED)")),
    ],
)
def blackbox(ui, repo, **opts):
    """view recent repository events

    By default, show events in the last 15 minutes. Use '--start 60' to get
    events in the past hour.

    Use '--debug' to see raw JSON values instead of human-readable messages.

    Use '--pattern' to filter events by JSON patterns. Examples::

        # matches watchman events ("_" matches anything)
        {"watchman": "_"}

        # matches "ssh_getfiles" network operations that takes 10 to 100ms.
        {"network": {"op": "ssh_getfiles", "duration_ms": ["range", 10, 100]}}

        # matches pager, or editor, or pythonhook blocked events
        {"blocked": {"name": ["or", "pager", "editor", "pythonhook"]}}

        # matches process start events with non-root uid
        {"start": {"uid": ["not", 0]}}

        # matches start, or finish, or alias events
        ["or", {"start": "_"}, {"finish": "_"}, {"alias": "_"}]
    """
    # The source of truth of the JSON schema lives in Rust code:
    # blackbox/src/event.rs.

    start = opts.get("start", 15)
    end = opts.get("end", 0)
    showtimestamp = opts.get("timestamp", True)
    showsid = opts.get("sid", True)
    pattern = opts.get("pattern")

    now = time.time()
    events = filter(now - start * 60, now - end * 60 + 1, pattern)

    ui.pager("blackbox")
    sidcolor = {}
    debugflag = ui.debugflag
    for sid, ts, msg, json in reversed(events):
        if showtimestamp:
            localtime = time.localtime(ts)
            timestr = time.strftime("%Y/%m/%d %H:%M:%S", localtime) + (
                ".%03d" % (int(ts * 1000) % 1000)
            )
            ui.write(timestr, label="blackbox.timestamp")
            ui.write(" ")
        if showsid:
            color = sidcolor.get(sid)
            if color is None:
                color = len(sidcolor) % 4
                sidcolor[sid] = color
            if not debugflag:
                # The lowest 3 bytes are "pid". See blackbox.rs.
                sid = sid & 0xFFFFFF
            ui.write("%10d" % sid, label="blackbox.session.%d" % color)
            ui.write(" ")
        if debugflag:
            ui.write(json, label="blackbox.json")
        else:
            ui.write(msg.strip(), label="blackbox.message")
        ui.write("\n")

    # TODO: Consider properly templatize the output. So users can choose fields
    # to display, or format timestamp differently.
