# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# ownercheck.py - prevent operations on repos not owned

"""prevent operations on repos not owned by the current user

This extension checks the ownership of the local repo path (or its parent if
the path does not exist) and aborts if it does not match the current user.

This prevents some common mistakes like using sudo to clone a repo.
"""

import os
from typing import Optional

from edenscm import error, extensions, localrepo
from edenscm.i18n import _


try:
    import pwd
except ImportError:
    pwd = None


def _getowner(path):
    """find uid of a path or its parents. return (uid, path)"""
    path = os.path.abspath(path or "")
    while True:
        try:
            stat = os.stat(path)
            return stat.st_uid, path
        except Exception:
            parent = os.path.dirname(path)
            if parent == path:
                break
            path = parent
    return None, None


def _describeuser(uid):
    """convert uid to username if possible"""
    if pwd:
        try:
            return pwd.getpwuid(uid).pw_name
        except Exception:
            pass
    return "user %d" % uid


def _checkownedpath(path):
    ownerid, path = _getowner(path)
    uid = os.getuid()
    # allow access to public places owned by root (ex. /tmp)
    if ownerid in [None, 0, uid]:
        return
    raise error.Abort(
        _("%s is owned by %s, not you (%s).\n" "you are likely doing something wrong.")
        % (path, _describeuser(ownerid), _describeuser(uid)),
        hint=_("you can skip the check using " "--config extensions.ownercheck=!"),
    )


def _localrepoinit(
    orig, self, baseui, path, create=False, initial_config: Optional[str] = None
):
    _checkownedpath(path)
    return orig(self, baseui, path, create, initial_config)


def uisetup(ui):
    extensions.wrapfunction(localrepo.localrepository, "__init__", _localrepoinit)
