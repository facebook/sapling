# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import os
import subprocess

# Mercurial
from mercurial.i18n import _
from mercurial import (
    config,
    encoding,
    pycompat,
    util,
    vfs as vfsmod,
)

from . import commitcloudcommon

SERVICE = 'commitcloud'
ACCOUNT = 'commitcloud'

class TokenLocator(object):

    filename = '.commitcloudrc'

    def __init__(self, ui):
        self.ui = ui

    def _gettokenvfs(self):
        path = self.ui.config('commitcloud', 'user_token_path')
        if path and not os.path.isdir(path):
            raise commitcloudcommon.ConfigurationError(
                self.ui, _("invalid commitcloud.user_token_path '%s'") % path)
        if path:
            return vfsmod.vfs(util.expandpath(path))

        if pycompat.iswindows:
            envvar = 'APPDATA'
        else:
            envvar = 'HOME'
        homedir = encoding.environ.get(envvar)
        if not homedir:
            raise commitcloudcommon.ConfigurationError(
                self.ui, _('$%s environment variable not found') % envvar)

        if not os.path.isdir(homedir):
            raise commitcloudcommon.ConfigurationError(
                self.ui, _("invalid homedir '%s'") % homedir)

        return vfsmod.vfs(homedir)

    def _gettokenfromfile(self):
        """On platforms except macOS tokens are stored in a file"""
        vfs = self._gettokenvfs()
        if not os.path.exists(vfs.join(self.filename)):
            return None
        with vfs.open(self.filename, r'rb') as f:
            tokenconfig = config.config()
            tokenconfig.read(self.filename, f)
            return tokenconfig.get('commitcloud', 'user_token')
        return None

    def _settokentofile(self, token):
        """On platforms except macOS tokens are stored in a file"""
        vfs = self._gettokenvfs()
        with vfs.open(self.filename, 'w') as configfile:
            configfile.writelines(
                ['[commitcloud]\n', 'user_token=%s\n' % token])
        vfs.chmod(self.filename, 0o600)

    def _gettokenosx(self):
        """On macOS tokens are stored in keychain
           this function fetches token from keychain
        """
        p = subprocess.Popen(['security',
                              'find-generic-password',
                              '-g',
                              '-s',
                              SERVICE,
                              '-a',
                              ACCOUNT,
                              '-w'],
                             stdin=None,
                             close_fds=util.closefds,
                             stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT)
        try:
            text = p.stdout.read()
            if text:
                return text
            else:
                return None
        except Exception as e:
            raise commitcloudcommon.UnexpectedError(self.ui, e)

    def _settokenosx(self, token):
        """On macOS tokens are stored in keychain
           this function puts the token to keychain
        """
        p = subprocess.Popen(['security',
                              'add-generic-password',
                              '-a',
                              ACCOUNT,
                              '-s',
                              SERVICE,
                              '-p',
                              token,
                              '-U'],
                             stdin=None,
                             close_fds=util.closefds,
                             stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT)

        try:
            self.ui.debug('new token is stored in keychain\n')
            return p.stdout.read()
        except Exception as e:
            raise commitcloudcommon.UnexpectedError(self.ui, e)

    @property
    def token(self):
        """Public API
            get token
                returns None if token is not found
                it can throw only in case of unexpected error
        """
        if pycompat.isdarwin and not self.ui.config(
                'commitcloud', 'user_token_path'):
            return self._gettokenosx()
        else:
            return self._gettokenfromfile()

    def settoken(self, token):
        """Public API
            set token
                it can throw only in case of unexpected error
        """
        if pycompat.isdarwin and not self.ui.config(
                'commitcloud', 'user_token_path'):
            self._settokenosx(token)
        else:
            self._settokentofile(token)

# Changed in D7414658
def getdefaultworspace(ui):
    return ui.username()
