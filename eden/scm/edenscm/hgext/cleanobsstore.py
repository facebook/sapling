# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
extension for cleaning users' obsstore

this extension can be used for very narrow purpose. It deletes useless
obsmarkers from obsstore. 'Useless' is defined as marker whose username is in a
list of bad usernames (see config options below).
For example, if automation created lots of obsolete markers that were
accidentally pulled by many users then this extension can help. Probably the
better way to find useless markers is to check if precursor node is present in
the repo but it will be  slower.

Also note that this extension will run only once.

::

    [cleanobsstore]
    # list of usernames whose markers to delete from obsstore
    badusernames =
    # if obsstore size is bigger than obsstoresizelimit then we should try to
    # clean it
    obsstoresizelimit = 500000
"""

from __future__ import absolute_import, division, print_function

from edenscm.mercurial import obsutil, repair
from edenscm.mercurial.i18n import _


_cleanedobsstorefile = b"cleanedobsstore"


def reposetup(ui, repo):
    if repo.obsstore:
        indicestodelete = []
        if _needtoclean(ui, repo):
            repo._wlockfreeprefix.add(_cleanedobsstorefile)
            _write(ui, "your obsstore is big, checking if we can clean it")
            badusernames = ui.configlist("cleanobsstore", "badusernames")
            for index, data in enumerate(repo.obsstore._all):
                marker = obsutil.marker(repo, data)
                username = marker.metadata().get("user")
                if username:
                    if username in badusernames:
                        indicestodelete.append(index)
                # Intentionally marked as cleaned before the actual cleaning.
                # This is to avoid repeatedly iterating through the whole
                # obsstore for users who have many obsmarkers
                _markcleaned(repo)

        if indicestodelete:
            _write(
                ui,
                _(
                    "cleaning your obsstore to make hg faster; "
                    + "it is a one-time operation, please wait..."
                ),
            )
            with repo.lock():
                repair.deleteobsmarkers(repo.obsstore, indicestodelete)


def _needtoclean(ui, repo):
    obsstoresizelimit = ui.configint("cleanobsstore", "obsstoresizelimit", 500000)
    return repo.svfs.stat(
        "obsstore"
    ).st_size >= obsstoresizelimit and not repo.localvfs.exists(_cleanedobsstorefile)


def _markcleaned(repo):
    with repo.localvfs(_cleanedobsstorefile, "w") as f:
        f.write("1")  # any text will do


def _write(ui, msg):
    if ui.interactive() and not ui.plain("cleanobsstore"):
        ui.warn(_("%s\n") % msg)
