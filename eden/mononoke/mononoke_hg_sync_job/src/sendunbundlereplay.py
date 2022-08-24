# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# sendunbundlereplay.py - send unbundlereplay wireproto command
from __future__ import absolute_import

import base64
import contextlib
import json
import os
import sys

from edenscm.mercurial import error, hg, util
from edenscm.mercurial.commands import command
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import decodeutf8, encodeutf8


def getcommitdates(ui, fname=None):
    if fname:
        with open(fname, "r") as tf:
            timestamps = tf.readlines()
    else:
        timestamps = ui.fin
    return dict([s.split("=") for s in timestamps])


def gethgbonsaimapping(ui, fname):
    with open(fname, "r") as f:
        hgbonsaimapping = f.readlines()
        res = {}
        for s in hgbonsaimapping:
            hgcsid, bcsid = s.split("=")
            hgcsid = hgcsid.strip()
            bcsid = bcsid.strip()
            res[hgcsid] = bcsid
    return res


def getstream(fname):
    with open(fname, "rb") as f:
        return util.chunkbuffer([f.read()])


def getremote(ui, path):
    return hg.peer(ui, {}, path)


# Use a separate ReplayData class so that we can add/remove and don't depend
# on mercurial client release schedule
class ReplayData(object):
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


def runreplay(ui, remote, stream, commitdates, rebasedhead, ontobook, hgbonsaimapping):
    returncode = 0
    try:
        reply = remote.unbundlereplay(
            stream,
            [b"force"],
            remote.url(),
            ReplayData(commitdates, rebasedhead, ontobook, hgbonsaimapping),
            ui.configbool("sendunbundlereplay", "respondlightly", True),
        )
    except Exception:
        ui.warn(_("exception executing unbundlereplay on remote\n"))
        ui.traceback()
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
    reportsfile.write(encodeutf8(msg))
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
    "sendunbundlereplaybatch",
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

    Reads `(bundlefile, timestampsfile, hgbonsaimappingfname, ontobook, rebasedhead)` from
    stdin.
    `bundlefile` is a filename that contains a bundle that will be sent
    `timestampsfile` is a filename in <commithash>=<hg-parseable-date> format
    `hgbonsaimappingfname` is a filename in <hg_cs_id>=<bcs_id> format
    `ontobook` is the bookmark we are pushing to
    `rebasedhead` is the resulting hash

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

            parts = line.split()
            (
                bfname,
                tsfname,
                hgbonsaimappingfname,
                ontobook,
                rebasedhead,
                logfile,
            ) = parts
            ontobook = decodeutf8(base64.b64decode(ontobook))

            rebasedhead = None if rebasedhead == "DELETED" else rebasedhead
            commitdates = getcommitdates(ui, tsfname)
            hgbonsaimapping = gethgbonsaimapping(ui, hgbonsaimappingfname)
            stream = getstream(bfname)

            with capturelogs(ui, remote, logfile):
                returncode = runreplay(
                    ui,
                    remote,
                    stream,
                    commitdates,
                    rebasedhead,
                    ontobook,
                    hgbonsaimapping,
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
