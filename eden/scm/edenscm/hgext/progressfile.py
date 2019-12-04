# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""allows users to have JSON progress bar information written to a path

Controlled by the `ui.progressfile` config. Mercurial will overwrite this file
each time the progress bar is updated. It is not affected by HGPLAIN since it
does not write to stdout.

The schema of this file is (JSON):

- topics: array of topics from oldest to newest. (last is always the active one)
- state: map of topic names to objects with keys:
    - topic (e.g. "changesets", "manifests")
    - pos: which item number out of <total> we're processing
    - total: total number of items (can change!)
    - unit: name of the type of unit being processed (e.g., "changeset")
    - item: the active item being processed (e.g., "changeset #5")
    - active: whether this is the currently active progress bar
    - units_per_sec: if active, how many <unit>s per sec we're processing
    - speed_str: if active, a human-readable string of how many <unit>s per sec
        we're processing
    - estimate_sec: an estimate of how much time is left, in seconds
    - estimate_str: if active, a human-readable string estimate of how much time
        is left (e.g. "2m30s")

config example::

    [progress]
    # Where to write progress information
    statefile = /some/path/to/file
    # Append to the progress file, rather than replace
    statefileappend = true
    # Set pid to a fixed value for testing purpose
    fakedpid = 42
"""

from __future__ import absolute_import

import json

from edenscm.mercurial import progress, registrar, util


testedwith = "ships-with-fb-hgext"

configtable = {}
configitem = registrar.configitem(configtable)

configitem("progress", "statefile", default="")

_pid = None


def writeprogress(self, progressfile, filemode, bars):
    topics = {}
    for index, bar in enumerate(bars):
        pos, item = progress._progvalue(bar.value)
        topic = bar._topic
        unit = bar._unit
        total = bar._total
        isactive = index == self._currentbarindex
        cullempty = lambda str: str if str else None
        info = {
            "topic": topic,
            "pos": pos,
            "total": total,
            "unit": cullempty(unit),
            "item": cullempty(item),
            "active": isactive,
            "units_per_sec": None,
            "speed_str": None,
            "estimate_sec": None,
            "estimate_str": None,
            "pid": _pid,
        }
        if isactive:
            speed = progress.estimatespeed(bar)
            remaining = progress.estimateremaining(bar) if total else None
            info["units_per_sec"] = cullempty(speed)
            info["estimate_sec"] = cullempty(remaining)
            info["speed_str"] = cullempty(progress.fmtspeed(speed, bar))
            info["estimate_str"] = cullempty(progress.fmtremaining(remaining))
        topics[topic] = info

    text = json.dumps(
        {"state": topics, "topics": [bar._topic for bar in bars]}, sort_keys=True
    )
    try:
        with open(progressfile, filemode) as f:
            f.write(text + "\n")
    except (IOError, OSError):
        pass


def uisetup(ui):
    progressfile = ui.config("progress", "statefile")
    append = ui.configbool("progress", "statefileappend", False)
    filemode = "a+" if append else "w+"
    if progressfile:
        global _pid
        _pid = ui.configint("progress", "fakedpid") or util.getpid()

        # pyre-fixme[11]: Annotation `__class__` is not defined as a type.
        class fileengine(progress._engine.__class__):
            def _show(self, now):
                super(fileengine, self)._show(now)
                writeprogress(self, progressfile, filemode, self._bars)

            def _complete(self):
                super(fileengine, self)._complete()
                writeprogress(self, progressfile, filemode, [])

        progress._engine.__class__ = fileengine
