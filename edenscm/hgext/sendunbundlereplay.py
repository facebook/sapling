# sendunbundlereplay.py - send unbundlereplay wireproto command
#
# Copyright 2019-present Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import contextlib
import datetime
import os
import sys

from edenscm.mercurial import error, hg, replay, util
from edenscm.mercurial.commands import command
from edenscm.mercurial.i18n import _


def getcommitdates(ui, fname=None):
    if fname:
        with open(fname, "r") as tf:
            timestamps = tf.readlines()
    else:
        timestamps = ui.fin
    return dict(map(lambda s: s.split("="), timestamps))


def getstream(fname):
    with open(fname, "rb") as f:
        return util.chunkbuffer([f.read()])


@util.timed(annotation="creating a peer took")
def getremote(ui, path):
    return hg.peer(ui, {}, path)


@util.timed(annotation="single wireproto command took")
def runreplay(ui, remote, stream, commitdates, rebasedhead, ontobook):
    returncode = 0
    try:
        reply = remote.unbundlereplay(
            stream,
            ["force"],
            remote.url(),
            replay.ReplayData(commitdates, rebasedhead, ontobook),
            ui.configbool("sendunbundlereplay", "respondlightly", True),
        )
    except Exception:
        returncode = 255
    finally:
        if returncode != 0:
            return returncode

    for part in reply.iterparts():
        part.read()
        if part.type.startswith("error:"):
            returncode = 1
            ui.warn(_("replay failed: %s\n") % part.type)
            if "message" in part.params:
                ui.warn(_("part message: %s\n") % (part.params["message"]))
    return returncode


def writereport(reportsfile, msg):
    reportsfile.write(msg)
    reportsfile.flush()
    os.fsync(reportsfile.fileno())


@contextlib.contextmanager
def capturelogs(ui, remote, logfile):
    if logfile is None:
        yield
    else:
        uis = [remote.ui, ui]
        for u in uis:
            u.pushbuffer(error=True, subproc=True)

        try:
            yield
        finally:
            output = "".join([u.popbuffer() for u in uis])
            ui.write_err(output)
            with open(logfile, "w") as f:
                f.write(output)


@command(
    "^sendunbundlereplaybatch",
    [
        ("", "path", "", _("hg server remotepath (ssh)"), ""),
        ("", "reports", "", _("a file for unbundereplay progress reports"), ""),
    ],
    _("[OPTION]..."),
    norepo=True,
)
def sendunbundlereplaybatch(ui, **opts):
    """Send a batch of unbundlereplay wireproto commands to a given server

    This exists to amortize the costs of `hg.peer` creation over multiple
    `unbundlereplay` calls.

    Reads `(bundlefile, timestampsfile, ontobook, rebasedhead)` from
    stdin. See docs of `sendunbundlereplay` for more details.

    Takes the `reports` argument on the command line. After each unbundlereplay
    command is successfully executed, will write and flush a single line
    into this file, thus reporting progress. File is truncated at the beginning
    of this function.

    ``sendunbundlereplay.respondlightly`` config option instructs the server
    to avoid sending large bundle2 parts back.
    """
    if not opts.get("reports"):
        raise error.Abort("--reports argument is required")
    path = opts["path"]
    returncode = 0
    remote = getremote(ui, path)
    ui.debug("using %s as a reports file\n" % opts["reports"])
    with open(opts["reports"], "wb", 0) as reportsfile:
        counter = 0
        while True:
            line = sys.stdin.readline()
            if line == "":
                break

            # The newest sync job sends 5 parameters, but older versions send 4.
            # We default the last parameter to None for compatibility.
            parts = line.split()
            if len(parts) == 4:
                parts.append(None)
            (bfname, tsfname, ontobook, rebasedhead, logfile) = parts

            rebasedhead = None if rebasedhead == "DELETED" else rebasedhead
            commitdates = getcommitdates(ui, tsfname)
            stream = getstream(bfname)

            with capturelogs(ui, remote, logfile):
                returncode = runreplay(
                    ui, remote, stream, commitdates, rebasedhead, ontobook
                )

            if returncode != 0:
                # the word "failed" is an identifier of failure, do not change
                failure = "unbundle replay batch item #%i failed\n" % counter
                ui.warn(failure)
                writereport(reportsfile, failure)
                break
            success = "unbundle replay batch item #%i successfully sent\n" % counter
            ui.warn(success)
            writereport(reportsfile, success)
            counter += 1

    return returncode


@command(
    "^sendunbundlereplay",
    [
        ("", "file", "", _("file to read bundle from"), ""),
        ("", "path", "", _("hg server remotepath (ssh)"), ""),
        ("r", "rebasedhead", "", _("expected rebased head hash"), ""),
        (
            "",
            "deleted",
            False,
            _("bookmark was deleted, can't be used with `--rebasedhead`"),
        ),
        ("b", "ontobook", "", _("expected onto bookmark for pushrebase"), ""),
    ],
    _("[OPTION]..."),
    norepo=True,
)
def sendunbundlereplay(ui, **opts):
    """Send unbundlereplay wireproto command to a given server

    Takes `rebasedhook` and `ontobook` arguments on the commmand
    line, and commit dates in stdin. The commit date format is:
    <commithash>=<hg-parseable-date>

    ``sendunbundlereplay.respondlightly`` config option instructs the server
    to avoid sending large bundle2 parts back.
    """
    fname = opts["file"]
    path = opts["path"]
    rebasedhead = opts["rebasedhead"]
    deleted = opts["deleted"]
    ontobook = opts["ontobook"]
    if rebasedhead and deleted:
        raise error.Abort("can't use `--rebasedhead` and `--deleted`")

    if not (rebasedhead or deleted):
        raise error.Abort("either `--rebasedhead` or `--deleted` should be used")

    commitdates = getcommitdates(ui)
    stream = getstream(fname)
    remote = getremote(ui, path)
    return runreplay(ui, remote, stream, commitdates, rebasedhead, ontobook)
