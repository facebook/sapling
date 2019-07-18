# hint.py - utilities to register hint messages
#
#  Copyright 2018 Facebook Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from . import rcutil, util
from .i18n import _


hinttable = {
    "hgignore-deprecate": lambda path: (
        (
            "hgignore format is being deprecated. "
            "Consider updating %s to gitignore format. "
            "Check fburl.com/gitignore to learn more."
        )
        % path
    ),
    "branch-command-deprecate": lambda: _(
        "'hg branch' command does not do what you want, and is being removed. "
        "It always prints 'default' for now. "
        "Check fburl.com/why-no-named-branches for details."
    ),
}
messages = []
triggered = set()


def loadhint(ui, extname, registrarobj):
    for name, func in registrarobj._table.iteritems():
        hinttable[name] = func


def loadhintconfig(ui):
    for name, message in ui.configitems("hint-definitions"):
        hinttable[name] = lambda *args, **kwargs: message


def trigger(name, *args, **kwargs):
    """Trigger a hint message. It will be shown at the end of the command."""
    func = hinttable.get(name)
    if func and name not in triggered:
        triggered.add(name)
        msg = func(*args, **kwargs)
        if msg:
            messages.append((name, msg.rstrip()))


def show(ui):
    """Show all triggered hint messages"""
    if ui.plain("hint"):
        return
    acked = ui.configlist("hint", "ack")
    if acked == ["*"]:

        def isacked(name):
            return True

    else:
        acked = set(acked)

        def isacked(name):
            return name in acked or ui.configbool("hint", "ack-%s" % name)

    names = []
    for name, msg in messages:
        if not isacked(name):
            ui.write_err(("%s\n") % msg.rstrip(), notice=_("hint[%s]") % name)
            names.append(name)
    if names and not isacked("hint-ack"):
        msg = _("use 'hg hint --ack %s' to silence these hints\n") % " ".join(names)
        ui.write_err(msg, notice=_("hint[%s]") % "hint-ack")
    messages[:] = []
    triggered.clear()


def silence(ui, names):
    """Silence given hints"""
    paths = rcutil.userrcpath()
    # In case there are multiple candidate paths, pick the one that exists.
    # Otherwise, use the first one.
    path = ([p for p in paths if os.path.exists(p)] + [paths[0]])[0]
    acked = ui.configlist("hint", "ack")
    for name in names:
        if name not in acked:
            acked.append(name)
    value = " ".join(util.shellquote(w) for w in acked)
    rcutil.editconfig(path, "hint", "ack", value)
