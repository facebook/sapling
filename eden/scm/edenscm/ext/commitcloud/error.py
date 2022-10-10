# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""commit cloud errors"""

from __future__ import absolute_import

import traceback

from edenscm import error
from edenscm.i18n import _


def getsupportcontact(ui):
    return ui.label(
        ui.config("ui", "supportcontact", "the Source Control Team"),
        "commitcloud.team",
    )


class UnexpectedError(error.Abort):
    def __init__(self, ui, message, *args):
        details = traceback.format_exc()  # last part of traceback
        contact = _("(please contact %s to report the error)") % getsupportcontact(ui)
        message = "unexpected error: %s\n%s\n%s" % (message, details, contact)
        ui.log("commitcloud_error", commitcloud_sync_error="unexpected error")
        super(UnexpectedError, self).__init__(message, *args, component="commitcloud")


class RegistrationError(error.Abort):
    def __init__(self, ui, message, *args, **kwargs):
        details = ""
        authenticationhelp = ui.config("commitcloud", "auth_help")
        if authenticationhelp:
            details += (
                _("authentication instructions:\n%s") % authenticationhelp.strip()
            )
        contact = _(
            "(please contact %s if you are unable to authenticate)"
        ) % getsupportcontact(ui)
        message = "registration error: %s\n%s\n%s" % (message, details, contact)
        ui.log("commitcloud_error", commitcloud_sync_error="registration error")
        super(RegistrationError, self).__init__(
            message, *args, component="commitcloud", **kwargs
        )


class WorkspaceError(error.Abort):
    def __init__(self, ui, message, *args):
        details = _(
            "(your repo is not connected to any workspace)\n"
            "(use '@prog@ cloud join --help' for more details)"
        )
        message = "workspace error: %s\n%s" % (message, details)
        ui.log("commitcloud_error", commitcloud_sync_error="workspace error")
        super(WorkspaceError, self).__init__(message, *args, component="commitcloud")


class ConfigurationError(error.Abort):
    def __init__(self, ui, message, *args):
        message = "config error: %s" % (message,)
        ui.log("commitcloud_error", commitcloud_sync_error="config error")
        super(ConfigurationError, self).__init__(
            message, *args, component="commitcloud"
        )


class TLSConfigurationError(error.Abort):
    def __init__(self, ui, message, *args):
        # internal config: help.tlsauthhelp
        helptext = ui.config("help", "tlsauthhelp")
        message = "TLS config error: %s" % (message,)
        if helptext:
            message += "\n" + helptext
        ui.log("commitcloud_error", commitcloud_sync_error="TLS config error")
        super(TLSConfigurationError, self).__init__(
            message, *args, component="commitcloud"
        )


class BadRequestError(error.Abort):
    def __init__(self, ui, message, *args):
        ui.log("commitcloud_error", commitcloud_sync_error="bad request error")
        super(BadRequestError, self).__init__(message, *args, component="commitcloud")


class ServiceError(error.Abort):
    def __init__(self, ui, message, *args):
        helptext = _(
            "(retry might help, please contact %s if this error persists)"
        ) % getsupportcontact(ui)
        message = "service error: %s\n%s" % (message, helptext)
        ui.log("commitcloud_error", commitcloud_sync_error="service error")
        super(ServiceError, self).__init__(message, *args, component="commitcloud")


class InvalidWorkspaceDataError(error.Abort):
    def __init__(self, ui, message, *args):
        details = _("(please run '@prog@ cloud recover')")
        message = "invalid workspace data: '%s'\n%s" % (message, details)
        ui.log("commitcloud_error", commitcloud_sync_error="invalid workspace data")
        super(InvalidWorkspaceDataError, self).__init__(
            message, *args, component="commitcloud"
        )


class SynchronizationError(error.Abort):
    def __init__(self, ui, message, *args):
        details = _("(please retry '@prog@ cloud sync')")
        contact = _("(please contact %s if this error persists)") % getsupportcontact(
            ui
        )
        message = "failed to synchronize commits: '%s'\n%s\n%s" % (
            message,
            details,
            contact,
        )
        ui.log("commitcloud_error", commitcloud_sync_error="synchronization error")
        super(SynchronizationError, self).__init__(
            message, *args, component="commitcloud"
        )


class SubprocessError(error.Abort):
    def __init__(self, ui, rc, stderrdata, *args):
        message = _("process exited with status %d") % rc
        contact = _("(please contact %s to report the error)") % getsupportcontact(ui)
        message = "subprocess error: '%s'\n%s\n%s" % (
            message,
            stderrdata.strip(),
            contact,
        )
        ui.log("commitcloud_error", commitcloud_sync_error="subprocess error")
        super(SubprocessError, self).__init__(message, *args, component="commitcloud")


class KeychainAccessError(error.Abort):
    def __init__(self, ui, reason, solution, *args):
        contact = _("(please contact %s if this error persists)") % getsupportcontact(
            ui
        )
        message = "keychain access error: '%s'\n%s\n%s" % (reason, solution, contact)
        ui.log("commitcloud_error", commitcloud_sync_error="keychain access error")
        super(KeychainAccessError, self).__init__(
            message, *args, component="commitcloud"
        )


class TLSAccessError(error.Abort):
    def __init__(self, ui, reason, *args):
        # internal config: help.tlshelp
        helptext = ui.config("help", "tlshelp")
        contact = _("(please contact %s if this error persists)") % getsupportcontact(
            ui
        )
        message = "TLS error: '%s'\n" % reason
        if helptext:
            message += "\n" + helptext
        message += "\n" + contact
        ui.log("commitcloud_error", commitcloud_sync_error="TLS access error")
        super(TLSAccessError, self).__init__(message, *args, component="commitcloud")
