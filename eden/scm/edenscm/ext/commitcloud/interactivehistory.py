# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import threading
import time
import traceback

from bindings import sptui
from edenscm import cmdutil, error, progress, util
from edenscm.i18n import _

from . import service


def showhistory(ui, repo, reponame, workspacename, templatealias, **opts) -> None:
    class cloudsl(object):
        def __init__(self, ui, repo, reponame, workspacename, **opts):
            self.ui = ui
            self.repo = repo
            self.reponame = reponame
            self.workspacename = workspacename
            self.opts = opts
            self.serv = service.get(ui)
            self.servlock = threading.Lock()
            self.renderevent = threading.Event()
            self.running = True
            self.cache = {}
            with progress.spinner(ui, _("fetching cloud smartlog history")):
                self.versions = sorted(
                    self.serv.gethistoricalversions(reponame, workspacename),
                    key=lambda version: version["version_number"],
                )

            initversion = opts.get("workspace_version")
            date = opts.get("date")
            inittime = int(util.parsedate(date)[0]) if date else None

            if initversion and inittime:
                raise error.Abort(
                    "'--workspace-version' and '--date' options can't be both provided"
                )

            if inittime:
                timestamps = sorted(
                    self.versions,
                    key=lambda version: version["timestamp"],
                )
                for index, version in enumerate(timestamps):
                    if version["timestamp"] >= inittime:
                        initversion = version["version_number"]
                        break
                    if index == len(timestamps) - 1:
                        raise error.Abort(
                            "You have no recorded history at or after this date"
                        )

            if not self.opts.get("template"):
                smartlogstyle = ui.config("templatealias", templatealias)
                if smartlogstyle:
                    self.opts["template"] = "{%s}" % smartlogstyle

            if initversion:
                initversion = int(initversion)
                for index, version in enumerate(self.versions):
                    if version["version_number"] == initversion:
                        self.cur_index = index
                        break
                else:
                    versionrange = [
                        version["version_number"] for version in self.versions
                    ]
                    raise error.Abort(
                        "workspace version %s is not available (%s to %s are available)"
                        % (initversion, min(versionrange), max(versionrange))
                    )
            else:
                self.cur_index = len(self.versions)
            if opts.get("all"):
                self.limit = 0
            else:
                self.limit = 12 * 7 * 24 * 60 * 60  # 12 weeks
            self.flags = []
            if ui.configbool("commitcloud", "sl_showremotebookmarks"):
                self.flags.append("ADD_REMOTE_BOOKMARKS")
            if ui.configbool("commitcloud", "sl_showallbookmarks"):
                self.flags.append("ADD_ALL_BOOKMARKS")

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
