# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import subprocess

from edenscm.mercurial import config, pycompat, util, vfs as vfsmod

from . import error as ccerror, util as ccutil


class TokenLocator(object):

    filename = ".commitcloudrc"
    servicename = "commitcloud"
    accountname = "commitcloud"

    def __init__(self, ui):
        self.ui = ui
        self.vfs = vfsmod.vfs(ccutil.getuserconfigpath(self.ui, "user_token_path"))
        self.vfs.createmode = 0o600
        # using platform username
        self.secretname = (self.servicename + "_" + util.getuser()).upper()
        self.usesecretstool = self.ui.configbool("commitcloud", "use_secrets_tool")

    def _gettokenfromfile(self):
        """On platforms except macOS tokens are stored in a file"""
        if not self.vfs.exists(self.filename):
            if self.usesecretstool:
                # check if token has been backed up and recover it if possible
                try:
                    token = self._gettokenfromsecretstool()
                    if token:
                        self._settokentofile(token, isbackedup=True)
                    return token
                except Exception:
                    pass
            return None

        with self.vfs.open(self.filename, r"rb") as f:
            tokenconfig = config.config()
            tokenconfig.read(self.filename, f)
            token = tokenconfig.get("commitcloud", "user_token")
            if self.usesecretstool:
                isbackedup = tokenconfig.get("commitcloud", "backedup")
                if not isbackedup:
                    self._settokentofile(token)
            return token

    def _settokentofile(self, token, isbackedup=False):
        """On platforms except macOS tokens are stored in a file"""
        # backup token if optional backup is enabled
        if self.usesecretstool and not isbackedup:
            try:
                self._settokeninsecretstool(token)
                isbackedup = True
            except Exception:
                pass
        with self.vfs.open(self.filename, "w") as configfile:
            configfile.write(
                "[commitcloud]\nuser_token=%s\nbackedup=%s\n" % (token, isbackedup)
            )

    def _gettokenfromsecretstool(self):
        """Token stored in keychain as individual secret"""
        try:
            p = subprocess.Popen(
                ["secrets_tool", "get", self.secretname],
                close_fds=util.closefds,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            (stdoutdata, stderrdata) = p.communicate()
            rc = p.returncode
            if rc != 0:
                return None
            text = stdoutdata.strip()
            return text or None

        except OSError as e:
            raise ccerror.UnexpectedError(self.ui, e)
        except ValueError as e:
            raise ccerror.UnexpectedError(self.ui, e)

    def _settokeninsecretstool(self, token, update=False):
        """Token stored in keychain as individual secrets"""
        action = "update" if update else "create"
        try:
            p = subprocess.Popen(
                [
                    "secrets_tool",
                    action,
                    "--read_contents_from_stdin",
                    self.secretname,
                    "Mercurial commitcloud token",
                ],
                close_fds=util.closefds,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                stdin=subprocess.PIPE,
            )
            (stdoutdata, stderrdata) = p.communicate(token)
            rc = p.returncode

            if rc != 0:
                if action == "create":
                    # Try updating token instead
                    self._settokeninsecretstool(token, update=True)
                else:
                    raise ccerror.SubprocessError(self.ui, rc, stderrdata)

            else:
                self.ui.debug(
                    "access token is backup up in secrets tool in %s\n"
                    % self.secretname
                )

        except OSError as e:
            raise ccerror.UnexpectedError(self.ui, e)
        except ValueError as e:
            raise ccerror.UnexpectedError(self.ui, e)

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
    def token(self):
        """Public API
            get token
                returns None if token is not found
                "not-required" is token is not needed
                it can throw only in case of unexpected error
        """
        if self.ui.configbool("commitcloud", "tls.notoken"):
            return "not-required"
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
