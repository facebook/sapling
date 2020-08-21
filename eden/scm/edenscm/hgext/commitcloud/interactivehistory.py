# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import time

from edenscm.mercurial import cmdutil, graphmod, progress
from edenscm.mercurial.i18n import _

from .. import interactiveui
from . import service, token as tokenmod, util as ccutil, workspace


def showhistory(ui, repo, reponame, workspacename, **opts):
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
            cmdutil.rustdisplaygraph(ui, repo, revdag, displayer, reserved=firstpublic)
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
