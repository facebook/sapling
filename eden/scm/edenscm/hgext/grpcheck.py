# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# grpcheck.py - check if the user is in specified groups

"""check if the user is in specified groups

If the user is not a member of specified groups, optionally show a warning
message and override some other config items.

::
    [grpcheck]
    # the user is expected to be a member of devs and users group
    groups = devs, users
    # warning to show if the user is not a member of any of those groups
    warning = You are not a member of %s group. Consult IT department for help.
    # if the user is not a member of any of those groups, override chgserver
    # config to make chgserver exit earlier
    overrides.chgserver.idletimeout = 2
"""

import grp
import os

from edenscm.mercurial import registrar


testedwith = "ships-with-fb-hgext"

_missinggroup = None

configtable = {}
configitem = registrar.configitem(configtable)

configitem("grpcheck", "groups", default=[])


def _grpname2gid(name):
    try:
        return grp.getgrnam(name).gr_gid
    except KeyError:
        return None


def _firstmissinggroup(groupnames):
    usergids = set(os.getgroups())
    for name in groupnames:
        expectedgid = _grpname2gid(name)
        # ignore unknown groups
        if expectedgid is not None and expectedgid not in usergids:
            return name


def _overrideconfigs(ui):
    for k, v in ui.configitems("grpcheck"):
        if not k.startswith("overrides."):
            continue
        section, name = k[len("overrides.") :].split(".", 1)
        ui.setconfig(section, name, v)


def extsetup(ui):
    groupnames = ui.configlist("grpcheck", "groups")
    if not groupnames:
        return
    missing = _firstmissinggroup(groupnames)
    if not missing:
        return
    message = ui.config("grpcheck", "warning")
    if message and not ui.plain():
        if "%s" in message:
            message = message % missing
        ui.warn(message + "\n")
    _overrideconfigs(ui)
    # re-used by reposetup. groups information is immutable for a process,
    # so we can re-use the "missing" calculation result safely.
    global _missinggroup
    _missinggroup = missing


def reposetup(ui, repo):
    if _missinggroup:
        _overrideconfigs(ui)
