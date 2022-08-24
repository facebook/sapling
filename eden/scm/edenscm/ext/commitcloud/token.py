# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import subprocess

from edenscm import config, pycompat, util, vfs as vfsmod

from . import error as ccerror, util as ccutil


class TokenLocator(object):

    filename = ".commitcloudrc"
    servicename = "commitcloud"
    accountname = "commitcloud"

    def __init__(self, ui):
        self.ui = ui
        self.vfs = vfsmod.vfs(ccutil.getuserconfigpath(self.ui, "user_token_path"))
        self.vfs.createmode = 0o600

    def _gettokenfromfile(self):
        """On platforms except macOS tokens are stored in a file"""
        if not self.vfs.exists(self.filename):
            return None

        with self.vfs.open(self.filename, r"rb") as f:
            tokenconfig = config.config()
            tokenconfig.read(self.filename, f)
            token = tokenconfig.get("commitcloud", "user_token")
            return token

    def _settokentofile(self, token, isbackedup=False):
        """On platforms except macOS tokens are stored in a file"""
        with self.vfs.open(self.filename, "wb") as configfile:
            configfile.write(
                b"[commitcloud]\nuser_token=%s\nbackedup=%s\n"
                % (pycompat.encodeutf8(token), pycompat.encodeutf8(str(isbackedup)))
            )

    def _gettokenosx(self):
        """On macOS tokens are stored in keychain
        this function fetches token from keychain
        """
        try:
            args = [
                "security",
                "find-generic-password",
                "-g",
                "-s",
                self.servicename,
                "-a",
                self.accountname,
                "-w",
            ]
            p = subprocess.Popen(
                args,
                stdin=None,
                close_fds=util.closefds,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            (stdoutdata, stderrdata) = p.communicate()
            rc = p.returncode
            if rc != 0:
                # security command is unable to show a prompt
                # from ssh sessions
                if rc == 36:
                    raise ccerror.KeychainAccessError(
                        self.ui,
                        "failed to access your keychain",
                        "please run 'security unlock-keychain' "
                        "to prove your identity\n"
                        "the command '%s' exited with code %d" % (" ".join(args), rc),
                    )
                # if not found, not an error
                if rc == 44:
                    return None
                raise ccerror.SubprocessError(
                    self.ui,
                    rc,
                    "command: '%s'\nstderr: %s" % (" ".join(args), stderrdata),
                )
            text = stdoutdata.strip()
            if text:
                return text
            else:
                return None
        except OSError as e:
            raise ccerror.UnexpectedError(self.ui, e)
        except ValueError as e:
            raise ccerror.UnexpectedError(self.ui, e)

    def _settokenosx(self, token):
        """On macOS tokens are stored in keychain
        this function puts the token to keychain
        """
        try:
            args = [
                "security",
                "add-generic-password",
                "-a",
                self.accountname,
                "-s",
                self.servicename,
                "-w",
                token,
                "-U",
            ]
            p = subprocess.Popen(
                args,
                stdin=None,
                close_fds=util.closefds,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            (stdoutdata, stderrdata) = p.communicate()
            rc = p.returncode
            if rc != 0:
                # security command is unable to show a prompt
                # from ssh sessions
                if rc == 36:
                    raise ccerror.KeychainAccessError(
                        self.ui,
                        "failed to access your keychain",
                        "please run 'security unlock-keychain' to prove your identity\n"
                        "the command '%s' exited with code %d" % (" ".join(args), rc),
                    )
                raise ccerror.SubprocessError(
                    self.ui,
                    rc,
                    "command: '%s'\nstderr: %s"
                    % (" ".join(args).replace(token, "<token>"), stderrdata),
                )
            self.ui.debug("new token is stored in keychain\n")
        except OSError as e:
            raise ccerror.UnexpectedError(self.ui, e)
        except ValueError as e:
            raise ccerror.UnexpectedError(self.ui, e)

    @property
    def tokenenforced(self):
        return self.ui.configbool("commitcloud", "token_enforced")

    @property
    def token(self):
        """Public API
        get token
            returns None if token is not found
            'faketoken' is token is not enforced
            it can throw only in case of unexpected error
        """
        if not self.tokenenforced:
            return ccutil.FAKE_TOKEN
        if self.ui.config("commitcloud", "user_token_path"):
            token = self._gettokenfromfile()
        elif pycompat.isdarwin:
            token = self._gettokenosx()
        else:
            token = self._gettokenfromfile()

        # Ensure token doesn't have any extraneous whitespace around it.
        if token is not None:
            token = token.strip()
        return token

    def settoken(self, token):
        """Public API
        set token
            it can throw only in case of unexpected error
        """
        # Ensure token doesn't have any extraneous whitespace around it.
        if token is not None:
            token = token.strip()

        if self.ui.config("commitcloud", "user_token_path"):
            self._settokentofile(token)
        elif pycompat.isdarwin:
            self._settokenosx(token)
        else:
            self._settokentofile(token)
