# pushvars.py - enable pushing environment variables to the server
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# When we want to modify how a hook works or to disable it, we
# can send environment variables that the hooks can read. The
# environment variables are prepended with HG_USERVAR_ and are thus
# not a security vulnerability. For example:
#
# hg push --pushvars="BYPASS_REVIEWERS=true" --pushvars="DEBUG=1"
#
# will result in both HG_USERVAR_BYPASS_REVIEWERS and HG_USERVAR_DEBUG
# available to the hook environment.

from mercurial import bundle2, cmdutil, exchange, extensions, hg
from mercurial import util, error, commands
from mercurial.i18n import _
from mercurial.extensions import _order
import errno, urllib

cmdtable = {}
command = cmdutil.command(cmdtable)

@exchange.b2partsgenerator('pushvars')
def _getbundlesendvars(pushop, bundler):
    '''send shellvars via bundle2'''
    if getattr(pushop.repo, '_shellvars', ()):
        part = bundler.newpart('pushvars')
        for entry in pushop.repo._shellvars:
            try:
                key, value = entry.split('=', 1)
            except Exception, e:
                raise util.Abort(
                    _('passed in variable needs to be of form var= or var=val. '
                      'Instead, this was given "%s"' % entry))
            part.addparam(key, value, mandatory=False)

# Ugly hack suggested by pyd to ensure pushvars part comes before
# hook part. Pyd has a fix for this in in he works.
exchange.b2partsgenorder.insert(0, exchange.b2partsgenorder.pop())

# Eventually, this will be used when we update to an  Hg that supports this.
#@exchange.b2partsgenerator('pushvars', idx=0)

@bundle2.parthandler('pushvars')
def bundle2getvars(op, part):
    '''unbundle a bundle2 containing shellvars on the server'''
    tr = op.gettransaction()
    for key, value in part.advisoryparams:
        key = key.upper()
        # We want pushed variables to have USERVAR_ prepended so we know
        # they came from the pushvar extension.
        key = "USERVAR_" + key
        tr.hookargs[key] = value

def push(orig, ui, repo, *args, **opts):
  repo._shellvars = opts['pushvars']
  try:
    return orig(ui, repo, *args, **opts)
  finally:
    del repo._shellvars

def uisetup(ui):
    # remotenames circumvents the default push implementation entirely, so make
    # sure we load after it so that we wrap it.
    order = list(extensions._order)
    order.remove('pushvars')
    order.append('pushvars')
    extensions._order = order

def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, 'push', push)
    entry[1].append(('', 'pushvars', [], "variables that can be sent to the server"))
