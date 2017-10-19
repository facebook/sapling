# hgweb/__init__.py - web interface to a mercurial repository
#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

from ..i18n import _

from .. import (
    error,
    pycompat,
    util,
)

from . import (
    hgweb_mod,
    hgwebdir_mod,
    server,
)

def hgweb(config, name=None, baseui=None):
    '''create an hgweb wsgi object

    config can be one of:
    - repo object (single repo view)
    - path to repo (single repo view)
    - path to config file (multi-repo view)
    - dict of virtual:real pairs (multi-repo view)
    - list of virtual:real tuples (multi-repo view)
    '''

    if ((isinstance(config, str) and not os.path.isdir(config)) or
        isinstance(config, dict) or isinstance(config, list)):
        # create a multi-dir interface
        return hgwebdir_mod.hgwebdir(config, baseui=baseui)
    return hgweb_mod.hgweb(config, name=name, baseui=baseui)

def hgwebdir(config, baseui=None):
    return hgwebdir_mod.hgwebdir(config, baseui=baseui)

class httpservice(object):
    def __init__(self, ui, app, opts):
        self.ui = ui
        self.app = app
        self.opts = opts

    def init(self):
        util.setsignalhandler()
        self.httpd = server.create_server(self.ui, self.app)

        if self.opts['port'] and not self.ui.verbose:
            return

        if self.httpd.prefix:
            prefix = self.httpd.prefix.strip('/') + '/'
        else:
            prefix = ''

        port = r':%d' % self.httpd.port
        if port == r':80':
            port = r''

        bindaddr = self.httpd.addr
        if bindaddr == r'0.0.0.0':
            bindaddr = r'*'
        elif r':' in bindaddr: # IPv6
            bindaddr = r'[%s]' % bindaddr

        fqaddr = self.httpd.fqaddr
        if r':' in fqaddr:
            fqaddr = r'[%s]' % fqaddr
        if self.opts['port']:
            write = self.ui.status
        else:
            write = self.ui.write
        write(_('listening at http://%s%s/%s (bound to %s:%d)\n') %
              (pycompat.sysbytes(fqaddr), pycompat.sysbytes(port),
               prefix, pycompat.sysbytes(bindaddr), self.httpd.port))
        self.ui.flush()  # avoid buffering of status message

    def run(self):
        self.httpd.serve_forever()

def createapp(baseui, repo, webconf):
    if webconf:
        return hgwebdir_mod.hgwebdir(webconf, baseui=baseui)
    else:
        if not repo:
            raise error.RepoError(_("there is no Mercurial repository"
                                    " here (.hg not found)"))
        return hgweb_mod.hgweb(repo, baseui=baseui)
