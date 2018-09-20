# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import traceback

from mercurial import error
from mercurial.i18n import _


def highlightmsg(ui, msg):
    """
    The tag is used to highlight important messages from Commit Cloud
    """
    return "%s %s" % (ui.label("#commitcloud", "commitcloud.tag"), msg)


def getownerteam(ui):
    return ui.label(
        ui.config("commitcloud", "owner_team", "the Source Control Team"),
        "commitcloud.team",
    )


"""
commit cloud error wrappers
"""


class UnexpectedError(error.Abort):
    def __init__(self, ui, message, *args):
        topic = highlightmsg(ui, _("unexpected error"))
        details = traceback.format_exc()  # last part of traceback
        contact = _("please contact %s to report the error") % getownerteam(ui)
        message = "%s: %s\n%s\n%s" % (topic, message, details, contact)
        super(UnexpectedError, self).__init__(message, *args)


class RegistrationError(error.Abort):
    def __init__(self, ui, message, *args, **kwargs):
        authenticationhelp = ui.config("commitcloud", "auth_help")
        if authenticationhelp:
            topic = highlightmsg(ui, _("registration error"))
            details = _("authentication instructions:\n%s") % authenticationhelp.strip()
            command = _(
                "please read `hg cloud authenticate --help` for more information"
            )
            contact = _(
                "please contact %s if you are unable to authenticate"
            ) % getownerteam(ui)
            message = "%s: %s\n%s\n%s\n%s" % (topic, message, details, command, contact)
        super(RegistrationError, self).__init__(message, *args, **kwargs)


class WorkspaceError(error.Abort):
    def __init__(self, ui, message, *args):
        topic = highlightmsg(ui, _("workspace error"))
        details = _(
            "your repo is not connected to any workspace\n"
            "use 'hg cloud join --help' for more details"
        )
        message = "%s: %s\n%s" % (topic, message, details)
        super(WorkspaceError, self).__init__(message, *args)


class ConfigurationError(error.Abort):
    def __init__(self, ui, message, *args):
        topic = highlightmsg(ui, _("config error"))
        contact = _("please contact %s to report misconfiguration") % getownerteam(ui)
        message = "%s: %s\n%s" % (topic, message, contact)
        super(ConfigurationError, self).__init__(message, *args)


class ServiceError(error.Abort):
    """Commit Cloud errors from remote service"""

    def __init__(self, ui, message, *args):
        topic = highlightmsg(ui, _("error from the remote service"))
        host = ui.config("commitcloud", "remote_host")
        port = ui.configint("commitcloud", "remote_port")
        details = _("commit cloud endpoint is '%s:%d' (retry might help)") % (
            host,
            port,
        )
        contact = _("please contact %s if this error persists") % getownerteam(ui)
        message = "%s: %s\n%s\n%s" % (topic, message, details, contact)
        super(ServiceError, self).__init__(message, *args)


class InvalidWorkspaceDataError(error.Abort):
    def __init__(self, ui, message, *args):
        topic = highlightmsg(ui, _("invalid workspace data"))
        details = _("please run 'hg cloud recover'")
        message = "%s: '%s'\n%s" % (topic, message, details)
        super(InvalidWorkspaceDataError, self).__init__(message, *args)


class SynchronizationError(error.Abort):
    def __init__(self, ui, message, *args):
        topic = highlightmsg(ui, _("failed to synchronize commits"))
        details = _("please retry 'hg cloud sync'")
        contact = _("please contact %s if this error persists") % getownerteam(ui)
        message = "%s: '%s'\n%s\n%s" % (topic, message, details, contact)
        super(SynchronizationError, self).__init__(message, *args)


class SubprocessError(error.Abort):
    def __init__(self, ui, rc, stderrdata, *args):
        topic = highlightmsg(ui, _("subprocess error"))
        message = _("process exited with status %d") % rc
        contact = _("please contact %s to report the error") % getownerteam(ui)
        message = "%s: '%s'\n%s\n%s" % (topic, message, stderrdata.strip(), contact)
        super(SubprocessError, self).__init__(message, *args)


class KeychainAccessError(error.Abort):
    def __init__(self, ui, reason, solution, *args):
        topic = highlightmsg(ui, _("keychain access error"))
        contact = _("please contact %s if this error persists") % getownerteam(ui)
        message = "%s: '%s'\n%s\n%s" % (topic, reason, solution, contact)
        super(KeychainAccessError, self).__init__(message, *args)


class TLSAccessError(error.Abort):
    def __init__(self, ui, reason, details, *args):
        topic = highlightmsg(ui, _("tls certificate error"))
        contact = _("please contact %s if this error persists") % getownerteam(ui)
        message = "%s: '%s'\n%s\n%s" % (topic, reason, "\n".join(details), contact)
        super(TLSAccessError, self).__init__(message, *args)


"""
commit cloud message wrappers
"""


def highlightstatus(ui, msg):
    ui.status(highlightmsg(ui, msg))


def highlightdebug(ui, msg):
    ui.debug(highlightmsg(ui, msg))
