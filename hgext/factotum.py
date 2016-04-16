# factotum.py - Plan 9 factotum integration for Mercurial
#
# Copyright (C) 2012 Steven Stallion <sstallion@gmail.com>
#
# This program is free software; you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation; either version 2 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General
# Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program; if not, write to the Free Software Foundation, Inc.,
# 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.

'''http authentication with factotum

This extension allows the factotum(4) facility on Plan 9 from Bell Labs
platforms to provide authentication information for HTTP access. Configuration
entries specified in the auth section as well as authentication information
provided in the repository URL are fully supported. If no prefix is specified,
a value of "*" will be assumed.

By default, keys are specified as::

  proto=pass service=hg prefix=<prefix> user=<username> !password=<password>

If the factotum extension is unable to read the required key, one will be
requested interactively.

A configuration section is available to customize runtime behavior. By
default, these entries are::

  [factotum]
  executable = /bin/auth/factotum
  mountpoint = /mnt/factotum
  service = hg

The executable entry defines the full path to the factotum binary. The
mountpoint entry defines the path to the factotum file service. Lastly, the
service entry controls the service name used when reading keys.

'''

from __future__ import absolute_import

import os
from mercurial.i18n import _
from mercurial import (
    error,
    httpconnection,
    url,
    util,
)

urlreq = util.urlreq
passwordmgr = url.passwordmgr

ERRMAX = 128

_executable = _mountpoint = _service = None

def auth_getkey(self, params):
    if not self.ui.interactive():
        raise error.Abort(_('factotum not interactive'))
    if 'user=' not in params:
        params = '%s user?' % params
    params = '%s !password?' % params
    os.system("%s -g '%s'" % (_executable, params))

def auth_getuserpasswd(self, getkey, params):
    params = 'proto=pass %s' % params
    while True:
        fd = os.open('%s/rpc' % _mountpoint, os.O_RDWR)
        try:
            os.write(fd, 'start %s' % params)
            l = os.read(fd, ERRMAX).split()
            if l[0] == 'ok':
                os.write(fd, 'read')
                status, user, passwd = os.read(fd, ERRMAX).split(None, 2)
                if status == 'ok':
                    if passwd.startswith("'"):
                        if passwd.endswith("'"):
                            passwd = passwd[1:-1].replace("''", "'")
                        else:
                            raise error.Abort(_('malformed password string'))
                    return (user, passwd)
        except (OSError, IOError):
            raise error.Abort(_('factotum not responding'))
        finally:
            os.close(fd)
        getkey(self, params)

def monkeypatch_method(cls):
    def decorator(func):
        setattr(cls, func.__name__, func)
        return func
    return decorator

@monkeypatch_method(passwordmgr)
def find_user_password(self, realm, authuri):
    user, passwd = urlreq.httppasswordmgrwithdefaultrealm.find_user_password(
        self, realm, authuri)
    if user and passwd:
        self._writedebug(user, passwd)
        return (user, passwd)

    prefix = ''
    res = httpconnection.readauthforuri(self.ui, authuri, user)
    if res:
        _, auth = res
        prefix = auth.get('prefix')
        user, passwd = auth.get('username'), auth.get('password')
    if not user or not passwd:
        if not prefix:
            prefix = realm.split(' ')[0].lower()
        params = 'service=%s prefix=%s' % (_service, prefix)
        if user:
            params = '%s user=%s' % (params, user)
        user, passwd = auth_getuserpasswd(self, auth_getkey, params)

    self.add_password(realm, authuri, user, passwd)
    self._writedebug(user, passwd)
    return (user, passwd)

def uisetup(ui):
    global _executable
    _executable = ui.config('factotum', 'executable', '/bin/auth/factotum')
    global _mountpoint
    _mountpoint = ui.config('factotum', 'mountpoint', '/mnt/factotum')
    global _service
    _service = ui.config('factotum', 'service', 'hg')
