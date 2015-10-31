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

        port = ':%d' % self.httpd.port
        if port == ':80':
            port = ''

        bindaddr = self.httpd.addr
        if bindaddr == '0.0.0.0':
            bindaddr = '*'
        elif ':' in bindaddr: # IPv6
            bindaddr = '[%s]' % bindaddr

        fqaddr = self.httpd.fqaddr
        if ':' in fqaddr:
            fqaddr = '[%s]' % fqaddr
        if self.opts['port']:
            write = self.ui.status
        else:
            write = self.ui.write
        write(_('listening at http://%s%s/%s (bound to %s:%d)\n') %
              (fqaddr, port, prefix, bindaddr, self.httpd.port))
        self.ui.flush()  # avoid buffering of status message

    def run(self):
        self.httpd.serve_forever()

def createservice(ui, repo, opts):
    # this way we can check if something was given in the command-line
    if opts.get('port'):
        opts['port'] = util.getport(opts.get('port'))

    alluis = set([ui])
    if repo:
        baseui = repo.baseui
        alluis.update([repo.baseui, repo.ui])
    else:
        baseui = ui
    webconf = opts.get('web_conf') or opts.get('webdir_conf')
    if webconf:
        # load server settings (e.g. web.port) to "copied" ui, which allows
        # hgwebdir to reload webconf cleanly
        servui = ui.copy()
        servui.readconfig(webconf, sections=['web'])
        alluis.add(servui)
    else:
        servui = ui

    optlist = ("name templates style address port prefix ipv6"
               " accesslog errorlog certificate encoding")
    for o in optlist.split():
        val = opts.get(o, '')
        if val in (None, ''): # should check against default options instead
            continue
        for u in alluis:
            u.setconfig("web", o, val, 'serve')

    if webconf:
        app = hgwebdir_mod.hgwebdir(webconf, baseui=baseui)
    else:
        if not repo:
            raise error.RepoError(_("there is no Mercurial repository"
                                    " here (.hg not found)"))
        app = hgweb_mod.hgweb(repo, baseui=baseui)
    return httpservice(servui, app, opts)
