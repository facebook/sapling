# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""commit cloud errors"""

from __future__ import absolute_import

import traceback

from edenscm.mercurial import error
from edenscm.mercurial.i18n import _


def getownerteam(ui):
    return ui.label(
        ui.config("commitcloud", "owner_team", "the Source Control Team"),
        "commitcloud.team",
    )


def getconfighelp(ui):
    # internal config: help.commitcloud-config-remediate
    return ui.config("help", "commitcloud-config-remediate")


class UnexpectedError(error.Abort):
    def __init__(self, ui, message, *args):
        details = traceback.format_exc()  # last part of traceback
        contact = _("(please contact %s to report the error)") % getownerteam(ui)
        message = "unexpected error: %s\n%s\n%s" % (message, details, contact)
        super(UnexpectedError, self).__init__(message, *args, component="commitcloud")


class RegistrationError(error.Abort):
    def __init__(self, ui, message, *args, **kwargs):
        details = ""
        authenticationhelp = ui.config("commitcloud", "auth_help")
        if authenticationhelp:
            details += (
                _("authentication instructions:\n%s\n") % authenticationhelp.strip()
            )
        details += _(
            "(please read 'hg cloud authenticate --help' for more information)"
        )
        contact = _(
            "(please contact %s if you are unable to authenticate)"
        ) % getownerteam(ui)
        message = "registration error: %s\n%s\n%s" % (message, details, contact)
        super(RegistrationError, self).__init__(
            message, *args, component="commitcloud", **kwargs
        )


class WorkspaceError(error.Abort):
    def __init__(self, ui, message, *args):
        details = _(
            "(your repo is not connected to any workspace)\n"
            "(use 'hg cloud join --help' for more details)"
        )
        message = "workspace error: %s\n%s" % (message, details)
        super(WorkspaceError, self).__init__(message, *args, component="commitcloud")


class ConfigurationError(error.Abort):
    def __init__(self, ui, message, *args):
        helptext = getconfighelp(ui)
        message = "config error: %s" % (message,)
        if helptext:
            message += "\n" + helptext
        super(ConfigurationError, self).__init__(
            message, *args, component="commitcloud"
        )


class ServiceError(error.Abort):
    """Commit Cloud errors from remote service"""

    def __init__(self, ui, message, *args):
        host = ui.config("commitcloud", "remote_host")
        port = ui.configint("commitcloud", "remote_port")
        details = _(
            "(the Commit Cloud service endpoint is '%s:%d' - retry might help)"
        ) % (host, port)
        contact = _("(please contact %s if this error persists)") % getownerteam(ui)
        message = "service error: %s\n%s\n%s" % (message, details, contact)
        super(ServiceError, self).__init__(message, *args, component="commitcloud")


class InvalidWorkspaceDataError(error.Abort):
    def __init__(self, ui, message, *args):
        details = _("(please run 'hg cloud recover')")
        message = "invalid workspace data: '%s'\n%s" % (message, details)
        super(InvalidWorkspaceDataError, self).__init__(
            message, *args, component="commitcloud"
        )


class SynchronizationError(error.Abort):
    def __init__(self, ui, message, *args):
        details = _("(please retry 'hg cloud sync')")
        contact = _("(please contact %s if this error persists)") % getownerteam(ui)
        message = "failed to synchronize commits: '%s'\n%s\n%s" % (
            message,
            details,
            contact,
        )
        super(SynchronizationError, self).__init__(
            message, *args, component="commitcloud"
        )


class SubprocessError(error.Abort):
    def __init__(self, ui, rc, stderrdata, *args):
        message = _("process exited with status %d") % rc
        contact = _("(please contact %s to report the error)") % getownerteam(ui)
        message = "subprocess error: '%s'\n%s\n%s" % (
            message,
            stderrdata.strip(),
            contact,
        )
        super(SubprocessError, self).__init__(message, *args, component="commitcloud")


class KeychainAccessError(error.Abort):
    def __init__(self, ui, reason, solution, *args):
        contact = _("(please contact %s if this error persists)") % getownerteam(ui)
        message = "keychain access error: '%s'\n%s\n%s" % (reason, solution, contact)
        super(KeychainAccessError, self).__init__(
            message, *args, component="commitcloud"
        )


class TLSAccessError(error.Abort):
    def __init__(self, ui, reason, details, *args):
        contact = _("(please contact %s if this error persists)") % getownerteam(ui)
        message = "tls certificate error: '%s'\n%s\n%s" % (
            reason,
            "\n".join(details),
            contact,
        )
        super(TLSAccessError, self).__init__(message, *args, component="commitcloud")
