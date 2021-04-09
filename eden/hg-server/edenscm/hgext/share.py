# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""share a common history between several working directories"""

from __future__ import absolute_import

from edenscm.mercurial import error, hg, registrar
from edenscm.mercurial.i18n import _


repository = hg.repository

cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"


@command(
    "share",
    [
        ("U", "noupdate", None, _("do not create a working directory")),
        ("B", "bookmarks", None, _("also share bookmarks")),
        (
            "",
            "relative",
            None,
            _("point to source using a relative path " "(EXPERIMENTAL)"),
        ),
    ],
    _("[-U] [-B] SOURCE [DEST]"),
    norepo=True,
)
def share(ui, source, dest=None, noupdate=False, bookmarks=False, relative=False):
    """create a new shared repository

    Initialize a new repository and working directory that shares its
    history (and optionally bookmarks) with another repository.

    .. note::

       using rollback or extensions that destroy/modify history (amend,
       rebase, etc.) can cause considerable confusion with shared
       clones. In particular, if two shared clones are both updated to
       the same changeset, and one of them destroys that changeset
       with rollback, the other clone will suddenly stop working: all
       operations will fail with "abort: working directory has unknown
       parent". The only known workaround is to use debugsetparents on
       the broken clone to reset it to a changeset that still exists.
    """

    hg.share(
        ui,
        source,
        dest=dest,
        update=not noupdate,
        bookmarks=bookmarks,
        relative=relative,
    )
    return 0


@command("unshare", [], "")
def unshare(ui, repo):
    """convert a shared repository to a normal one

    Copy the store data to the repo and remove the sharedpath data.
    """

    if not repo.shared():
        raise error.Abort(_("this is not a shared repo"))

    hg.unshare(ui, repo)
