# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import threading
import time
import traceback

from bindings import sptui
from edenscm.mercurial import cmdutil, graphmod, progress
from edenscm.mercurial.i18n import _

from .. import interactiveui
from . import service, token as tokenmod, util as ccutil, workspace


def showhistory(ui, repo, reponame, workspacename, template, **opts):
    class cloudsl(object):
        def __init__(self, ui, repo, reponame, workspacename, **opts):
            self.ui = ui
            self.repo = repo
            self.reponame = reponame
            self.workspacename = workspacename
            self.opts = opts
            self.serv = service.get(ui, tokenmod.TokenLocator(ui).token)
            self.servlock = threading.Lock()
            self.renderevent = threading.Event()
            self.running = True
            self.cache = {}
            with progress.spinner(ui, _("fetching cloud smartlog history")):
                self.versions = sorted(
                    self.serv.gethistoricalversions(reponame, workspacename),
                    key=lambda version: version["version_number"],
                )

            smartlogstyle = ui.config("templatealias", template)
            if smartlogstyle:
                self.opts["template"] = "{%s}" % smartlogstyle

            self.cur_index = len(self.versions)
            if opts.get("all"):
                self.limit = 0
            else:
                self.limit = 2 * 24 * 60 * 60  # two weeks
            self.flags = []

        def prevversion(self):
            if self.cur_index > 0:
                self.cur_index -= 1
                self.schedulerender()

        def nextversion(self):
            if self.cur_index < len(self.versions):
                self.cur_index += 1
                self.schedulerender()

        def schedulerender(self):
            self.renderevent.set()

        def renderloop(self):
            while self.running:
                self.renderevent.clear()
                self.render()
                self.renderevent.wait()

        def run(self):
            # Bind [ and ] to switch versions.  Unbind switching file (tab + shift-tab).
            self.tui = sptui.sptui(
                "Cloud Smartlog History",
                [
                    (
                        ("Navigation", "Go to previous version", self.prevversion),
                        [(sptui.NONE, "[")],
                    ),
                    (
                        ("Navigation", "Go to next version", self.nextversion),
                        [(sptui.NONE, "]")],
                    ),
                    (
                        None,
                        [(sptui.NONE, "Tab"), (sptui.SHIFT, "Tab")],
                    ),
                ],
            )

            # Start a background thread that will render the smartlog
            # versions into streampager.
            r = threading.Thread(target=self.renderloop, name="render")
            r.daemon = True
            r.start()

            # Pre-cache some recent versions in another background thread.
            t = threading.Thread(target=self.precache, name="precache")
            t.daemon = True
            t.start()

            # On the main thread, wait for the pager to terminate, and
            # then clean up.
            self.tui.wait()
            self.running = False
            self.renderevent.set()

        def precache(self):
            for i in range(3):
                v = len(self.versions) - 1 - i
                if v not in self.cache:
                    try:
                        self.cache[v] = self.rendercontents(v)
                    except Exception:
                        pass
                    time.sleep(0)

        def loadoldversion(self, versionindex):
            versionnumber = self.versions[versionindex]["version_number"]
            with self.servlock, progress.spinner(
                self.ui, _("fetching version %s") % versionnumber
            ):
                limit = self.limit
                if limit > 0:
                    # Increase the limit by how long ago the smartlog was
                    # backed-up.  This gives a rolling window, so viewing
                    # versions more than the limit in age will still show
                    # commits.
                    timestamp = self.versions[versionindex]["timestamp"]
                    limit += max(0, int(time.time() - timestamp))
                slinfo = self.serv.getsmartlogbyversion(
                    self.reponame,
                    self.workspacename,
                    self.repo,
                    None,
                    versionnumber,
                    limit,
                    self.flags,
                )
                formatteddate = time.strftime(
                    "%Y-%m-%d %H:%M:%S", time.localtime(slinfo.timestamp)
                )
                title = "Smartlog version %d synced at %s:" % (
                    slinfo.version,
                    formatteddate,
                )
                return (title, slinfo)

        def loadcurrentversion(self):
            with self.servlock:
                with progress.spinner(self.ui, _("fetching latest version")):
                    slinfo = self.serv.getsmartlog(
                        self.reponame,
                        self.workspacename,
                        self.repo,
                        self.limit,
                        self.flags,
                    )
                    title = "Current cloud smartlog"
                    return (title, slinfo)

        def render(self):
            versionindex = self.cur_index
            try:
                contents = self.rendercontents(versionindex)
                self.tui.replace_contents(contents)
            except Exception:
                if versionindex == len(self.versions):
                    error = "Failed to load latest smartlog"
                else:
                    versionnumber = self.versions[versionindex]["version_number"]
                    error = "Failed to load version %s" % versionnumber
                contents = [
                    error.encode(),
                    b"",
                    b"The error was:",
                    b"",
                ] + traceback.format_exc().encode().split(b"\n")
                self.tui.replace_contents(contents)

        def rendercontents(self, versionindex):
            if versionindex in self.cache:
                contents = self.cache[versionindex]
            else:
                if versionindex == len(self.versions):
                    (title, slinfo) = self.loadcurrentversion()
                else:
                    (title, slinfo) = self.loadoldversion(versionindex)

                contents = [
                    ui.label(
                        "Commit Cloud Smartlog History", "bold cyan underline"
                    ).encode(),
                    ui.label(
                        "Use [ and ] to navigate to earlier or later versions", "cyan"
                    ).encode(),
                    ui.label(
                        "Note: version dates may be off by one due to a server bug",
                        "cyan",
                    ).encode(),
                    b"",
                    title.encode(),
                    b"",
                ]
                firstpublic, revdag = self.serv.makedagwalker(slinfo, self.repo)
                displayer = cmdutil.show_changeset(
                    self.ui, self.repo, self.opts, buffered=True
                )

                def out(row):
                    contents.extend(row.rstrip().encode().split(b"\n"))

                with progress.spinner(ui, _("loading commit information")):
                    cmdutil.displaygraph(
                        self.ui,
                        self.repo,
                        revdag,
                        displayer,
                        reserved=firstpublic,
                        out=out,
                    )
                self.cache[versionindex] = contents

            return contents

    cloudsl(ui, repo, reponame, workspacename, **opts).run()


def oldshowhistory(ui, repo, reponame, workspacename, **opts):
    """Shows an interactive view for historical versions of smartlogs"""
    serv = service.get(ui, tokenmod.TokenLocator(ui).token)
    with progress.spinner(ui, _("fetching")):
        versions = sorted(
            serv.gethistoricalversions(reponame, workspacename),
            key=lambda version: version["version_number"],
            reverse=True,
        )

    class smartlogview(interactiveui.viewframe):
        def __init__(self, ui, repo, versions):
            interactiveui.viewframe.__init__(self, ui, repo, -1)
            self.versions = versions
            self.flags = []
            if opts.get("force_original_backend"):
                self.flags.append("USE_ORIGINAL_BACKEND")

        def render(self):
            ui = self.ui
            ui.pushbuffer()
            ui.status(_("Interactive Smartlog History\n\n"))
            if opts.get("all"):
                limit = 0
            else:
                limit = 2 * 604800  # two weeks
            if self.index == len(self.versions):
                self.index = -1
            if self.index == -2:
                self.index = len(self.versions) - 1
            if self.index == -1:
                with progress.spinner(ui, _("fetching")):
                    slinfo = serv.getsmartlog(
                        reponame, workspacename, repo, limit, self.flags
                    )
                ui.status(_("Current Smartlog:\n\n"))
            else:
                with progress.spinner(ui, _("fetching")):
                    slinfo = serv.getsmartlogbyversion(
                        reponame,
                        workspacename,
                        repo,
                        None,
                        self.versions[self.index]["version_number"],
                        limit,
                        self.flags,
                    )
                formatteddate = time.strftime(
                    "%Y-%m-%d %H:%M:%S", time.localtime(slinfo.timestamp)
                )
                ui.status(
                    _("Smartlog version %d \nsynced at %s\n\n")
                    % (slinfo.version, formatteddate)
                )
            template = "sl_cloud"
            smartlogstyle = ui.config("templatealias", template)
            if smartlogstyle:
                opts["template"] = "{%s}" % smartlogstyle
            else:
                ui.debug(
                    _("style %s is not defined, skipping") % smartlogstyle,
                    component="commitcloud",
                )

            firstpublic, revdag = serv.makedagwalker(slinfo, repo)
            displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
            cmdutil.displaygraph(ui, repo, revdag, displayer, reserved=firstpublic)
            repo.ui.status(
                _(
                    "<-: newer  "
                    "->: older  "
                    "q: abort  \n"
                    "a: 1 day forward  d: 1 day back \n"
                )
            )
            return ui.popbuffer()

        def rightarrow(self):
            self.index += 1

        def leftarrow(self):
            self.index -= 1

        def apress(self):
            if self.index == -1:
                return
            else:
                mintimestamp = self.versions[self.index]["timestamp"] + 86400
            while True:
                self.index -= 1
                if self.index <= -1:
                    break
                if self.versions[self.index]["timestamp"] >= mintimestamp:
                    break

        def dpress(self):
            if self.index == -1:
                maxtimestamp = int(time.time()) - 86400
            else:
                maxtimestamp = self.versions[self.index]["timestamp"] - 86400
            while True:
                self.index += 1
                if (
                    self.index == len(self.versions)
                    or self.versions[self.index]["timestamp"] <= maxtimestamp
                ):
                    break

        def enter(self):
            return

    viewobj = smartlogview(ui, repo, versions)
    interactiveui.view(viewobj)
