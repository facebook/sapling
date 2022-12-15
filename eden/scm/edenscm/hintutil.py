# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# hint.py - utilities to register hint messages

from __future__ import absolute_import

import os

from . import pycompat, rcutil, util
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
        "'@prog@ branch' command does not do what you want, and is being removed. "
        "It always prints 'default' for now. "
        "Check fburl.com/why-no-named-branches for details."
    ),
    "revnum-deprecate": lambda rev: _(
        "Local revision numbers (ex. %s) are being deprecated and will stop working in the future. "
        "Please use commit hashes instead."
    )
    % rev,
    "old-version": lambda agedays: _(
        f"WARNING! You version of @Product@ is {agedays} days old. Please upgrade your installation."
    ),
    "date-revset": lambda ds, top: _(
        'date("%s") performs a slow scan. Consider bsearch(date(">%s"),%s) instead.'
    )
    % (ds, ds, top),
    "match-full-traversal": lambda pats: _(
        'the patterns "%s" may be slow since they traverse the entire repo (see "@prog@ help patterns")',
    )
    % (pats),
}
messages = []
triggered = set()


def loadhint(ui, extname, registrarobj) -> None:
    for name, func in pycompat.iteritems(registrarobj._table):
        hinttable[name] = func


def loadhintconfig(ui) -> None:
    for name, message in ui.configitems("hint-definitions"):
        hinttable[name] = lambda *args, **kwargs: message


def trigger(name, *args, **kwargs) -> None:
    """Trigger a hint message. It will be shown at the end of the command."""
    func = hinttable.get(name)
    if func and name not in triggered:
        triggered.add(name)
        msg = func(*args, **kwargs)
        if msg:
            messages.append((name, msg.rstrip()))


def triggershow(ui, name, *args, **kwargs) -> None:
    """Trigger a hint message and show it immediately. Useful for warning
    things that might be slow before running the slow operation.
    """
    assert not isinstance(ui, str)
    func = hinttable.get(name)
    if func and name not in triggered and not isacked(ui, name):
        triggered.add(name)
        if not ui.plain("hint"):
            msg = func(*args, **kwargs)
            ui.write_err("%s\n" % msg.rstrip(), notice=_("hint[%s]") % name)


def show(ui) -> None:
    """Show all triggered hint messages"""
    if ui.plain("hint"):
        return
    names = []
    if util.mainio.is_pager_active():
        # For stream pager, people expect hints to be at the end, not in a
        # separate panel. Make it so. When the pager is active, we know that the
        # stdout is not being redirected to a file or pipe, so this won't affect
        # automation reading stdout.
        write = ui.write
    else:
        write = ui.write_err
    for name, msg in messages:
        if not isacked(ui, name):
            write("%s\n" % msg.rstrip(), notice=_("hint[%s]") % name)
            names.append(name)
    if names and not isacked(ui, "hint-ack"):
        msg = _("use '@prog@ hint --ack %s' to silence these hints\n") % " ".join(names)
        write(msg, notice=_("hint[%s]") % "hint-ack")
    messages[:] = []
    triggered.clear()


def silence(ui, names) -> None:
    """Silence given hints"""
    paths = ui.identity.userconfigpaths()
    # In case there are multiple candidate paths, pick the one that exists.
    # Otherwise, use the first one.
    path = ([p for p in paths if os.path.exists(p)] + [paths[0]])[0]
    acked = ui.configlist("hint", "ack")
    for name in names:
        if name not in acked:
            acked.append(name)
    value = " ".join(util.shellquote(w) for w in acked)
    rcutil.editconfig(path, "hint", "ack", value)


def clear() -> None:
    """Clear all triggered hints"""
    triggered.clear()
    del messages[:]


def isacked(ui, name):
    acked = ui.configlist("hint", "ack")
    if "*" in acked:
        return True
    else:
        return name in acked or ui.configbool("hint", "ack-%s" % name)
