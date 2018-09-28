# configwarn.py - warn unsupported user configs
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""warn unsupported user configs

Config::

    [configwarn]
    # Config names that are supposed to be set by system config and not
    # overrided by user config.
    systemconfigs = diff.git, extensions.hggit
"""

from __future__ import absolute_import

from mercurial import rcutil, registrar
from mercurial.i18n import _


configtable = {}
configitem = registrar.configitem(configtable)

configitem("configwarn", "systemconfigs", default=[])


def reposetup(ui, repo):
    # use reposetup, not uisetup to work better with chg and it checks reporc.
    if not repo.local():
        return

    nonsystempaths = set(rcutil.userrcpath() + [repo.localvfs.join("hgrc")])
    systemconfigs = ui.configlist("configwarn", "systemconfigs")

    for configname in systemconfigs:
        if "." not in configname:
            continue

        section, name = configname.split(".", 1)
        source = ui.configsource(section, name)

        if ":" not in source:
            continue

        path, lineno = source.split(":", 1)
        if path in nonsystempaths and lineno.isdigit():
            ui.warn(
                _(
                    "warning: overriding config %s is unsupported (hint: "
                    "remove line %s from %s to resolve this issue)\n"
                )
                % (configname, lineno, path)
            )
