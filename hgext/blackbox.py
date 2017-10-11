# blackbox.py - log repository events to a file for post-mortem debugging
#
# Copyright 2010 Nicolas Dumazet
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""log repository events to a blackbox for debugging

Logs event information to .hg/blackbox.log to help debug and diagnose problems.
The events that get logged can be configured via the blackbox.track config key.

Examples::

  [blackbox]
  track = *
  # dirty is *EXPENSIVE* (slow);
  # each log entry indicates `+` if the repository is dirty, like :hg:`id`.
  dirty = True
  # record the source of log messages
  logsource = True

  [blackbox]
  track = command, commandfinish, commandexception, exthook, pythonhook

  [blackbox]
  track = incoming

  [blackbox]
  # limit the size of a log file
  maxsize = 1.5 MB
  # rotate up to N log files when the current one gets too big
  maxfiles = 3

"""

from __future__ import absolute_import

import errno
import re

from mercurial.i18n import _
from mercurial.node import hex

from mercurial import (
    registrar,
    ui as uimod,
    util,
)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem('blackbox', 'dirty',
    default=False,
)
configitem('blackbox', 'maxsize',
    default='1 MB',
)
configitem('blackbox', 'logsource',
    default=False,
)
configitem('blackbox', 'maxfiles',
    default=7,
)
configitem('blackbox', 'track',
    default=lambda: ['*'],
)

lastui = None

def _openlogfile(ui, vfs):
    def rotate(oldpath, newpath):
        try:
            vfs.unlink(newpath)
        except OSError as err:
            if err.errno != errno.ENOENT:
                ui.debug("warning: cannot remove '%s': %s\n" %
                         (newpath, err.strerror))
        try:
            if newpath:
                vfs.rename(oldpath, newpath)
        except OSError as err:
            if err.errno != errno.ENOENT:
                ui.debug("warning: cannot rename '%s' to '%s': %s\n" %
                         (newpath, oldpath, err.strerror))

    maxsize = ui.configbytes('blackbox', 'maxsize')
    name = 'blackbox.log'
    if maxsize > 0:
        try:
            st = vfs.stat(name)
        except OSError:
            pass
        else:
            if st.st_size >= maxsize:
                path = vfs.join(name)
                maxfiles = ui.configint('blackbox', 'maxfiles')
                for i in xrange(maxfiles - 1, 1, -1):
                    rotate(oldpath='%s.%d' % (path, i - 1),
                           newpath='%s.%d' % (path, i))
                rotate(oldpath=path,
                       newpath=maxfiles > 0 and path + '.1')
    return vfs(name, 'a')

def wrapui(ui):
    class blackboxui(ui.__class__):
        @property
        def _bbvfs(self):
            vfs = None
            repo = getattr(self, '_bbrepo', None)
            if repo:
                vfs = repo.vfs
                if not vfs.isdir('.'):
                    vfs = None
            return vfs

        @util.propertycache
        def track(self):
            return self.configlist('blackbox', 'track')

        def log(self, event, *msg, **opts):
            global lastui
            super(blackboxui, self).log(event, *msg, **opts)

            if not '*' in self.track and not event in self.track:
                return

            if self._bbvfs:
                ui = self
            else:
                # certain ui instances exist outside the context of
                # a repo, so just default to the last blackbox that
                # was seen.
                ui = lastui

            if not ui:
                return
            vfs = ui._bbvfs
            if not vfs:
                return

            repo = getattr(ui, '_bbrepo', None)
            if not lastui or repo:
                lastui = ui
            if getattr(ui, '_bbinlog', False):
                # recursion and failure guard
                return
            ui._bbinlog = True
            default = self.configdate('devel', 'default-date')
            date = util.datestr(default, '%Y/%m/%d %H:%M:%S')
            user = util.getuser()
            pid = '%d' % util.getpid()
            formattedmsg = msg[0] % msg[1:]
            rev = '(unknown)'
            changed = ''
            if repo:
                ctx = repo[None]
                parents = ctx.parents()
                rev = ('+'.join([hex(p.node()) for p in parents]))
                if (ui.configbool('blackbox', 'dirty') and
                    ctx.dirty(missing=True, merge=False, branch=False)):
                    changed = '+'
            if ui.configbool('blackbox', 'logsource'):
                src = ' [%s]' % event
            else:
                src = ''
            try:
                fmt = '%s %s @%s%s (%s)%s> %s'
                args = (date, user, rev, changed, pid, src, formattedmsg)
                with _openlogfile(ui, vfs) as fp:
                    fp.write(fmt % args)
            except (IOError, OSError) as err:
                self.debug('warning: cannot write to blackbox.log: %s\n' %
                           err.strerror)
                # do not restore _bbinlog intentionally to avoid failed
                # logging again
            else:
                ui._bbinlog = False

        def setrepo(self, repo):
            self._bbrepo = repo

    ui.__class__ = blackboxui
    uimod.ui = blackboxui

def uisetup(ui):
    wrapui(ui)

def reposetup(ui, repo):
    # During 'hg pull' a httppeer repo is created to represent the remote repo.
    # It doesn't have a .hg directory to put a blackbox in, so we don't do
    # the blackbox setup for it.
    if not repo.local():
        return

    if util.safehasattr(ui, 'setrepo'):
        ui.setrepo(repo)

        # Set lastui even if ui.log is not called. This gives blackbox a
        # fallback place to log.
        global lastui
        if lastui is None:
            lastui = ui

    repo._wlockfreeprefix.add('blackbox.log')

@command('^blackbox',
    [('l', 'limit', 10, _('the number of events to show')),
    ],
    _('hg blackbox [OPTION]...'))
def blackbox(ui, repo, *revs, **opts):
    '''view the recent repository events
    '''

    if not repo.vfs.exists('blackbox.log'):
        return

    limit = opts.get('limit')
    fp = repo.vfs('blackbox.log', 'r')
    lines = fp.read().split('\n')

    count = 0
    output = []
    for line in reversed(lines):
        if count >= limit:
            break

        # count the commands by matching lines like: 2013/01/23 19:13:36 root>
        if re.match('^\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2} .*> .*', line):
            count += 1
        output.append(line)

    ui.status('\n'.join(reversed(output)))
