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

If you want to record whether the repository is dirty (a `+` sign, as you'd
get when using :hg:`id`), you can set the blackbox.dirty config key.

Examples::

  [blackbox]
  track = *
  dirty = True

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
    cmdutil,
    util,
)

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'
lastui = None

filehandles = {}

def _openlog(vfs):
    path = vfs.join('blackbox.log')
    if path in filehandles:
        return filehandles[path]
    filehandles[path] = fp = vfs('blackbox.log', 'a')
    return fp

def _closelog(vfs):
    path = vfs.join('blackbox.log')
    fp = filehandles[path]
    del filehandles[path]
    fp.close()

def hexfn(node):
    if node is None:
        return None
    else:
        return hex(node)

def wrapui(ui):
    class blackboxui(ui.__class__):
        @util.propertycache
        def track(self):
            return self.configlist('blackbox', 'track', ['*'])

        def _openlogfile(self):
            def rotate(oldpath, newpath):
                try:
                    self._bbvfs.unlink(newpath)
                except OSError as err:
                    if err.errno != errno.ENOENT:
                        self.debug("warning: cannot remove '%s': %s\n" %
                                   (newpath, err.strerror))
                try:
                    if newpath:
                        self._bbvfs.rename(oldpath, newpath)
                except OSError as err:
                    if err.errno != errno.ENOENT:
                        self.debug("warning: cannot rename '%s' to '%s': %s\n" %
                                   (newpath, oldpath, err.strerror))

            fp = _openlog(self._bbvfs)
            maxsize = self.configbytes('blackbox', 'maxsize', 1048576)
            if maxsize > 0:
                st = self._bbvfs.fstat(fp)
                if st.st_size >= maxsize:
                    path = fp.name
                    _closelog(self._bbvfs)
                    maxfiles = self.configint('blackbox', 'maxfiles', 7)
                    for i in xrange(maxfiles - 1, 1, -1):
                        rotate(oldpath='%s.%d' % (path, i - 1),
                               newpath='%s.%d' % (path, i))
                    rotate(oldpath=path,
                           newpath=maxfiles > 0 and path + '.1')
                    fp = _openlog(self._bbvfs)
            return fp

        def _bbwrite(self, fmt, *args):
            self._bbfp.write(fmt % args)
            self._bbfp.flush()

        def log(self, event, *msg, **opts):
            global lastui
            super(blackboxui, self).log(event, *msg, **opts)

            if not '*' in self.track and not event in self.track:
                return

            if util.safehasattr(self, '_bbfp'):
                ui = self
            elif util.safehasattr(self, '_bbvfs'):
                try:
                    self._bbfp = self._openlogfile()
                except (IOError, OSError) as err:
                    self.debug('warning: cannot write to blackbox.log: %s\n' %
                               err.strerror)
                    del self._bbvfs
                    self._bbfp = None
                ui = self
            else:
                # certain ui instances exist outside the context of
                # a repo, so just default to the last blackbox that
                # was seen.
                ui = lastui

            if (util.safehasattr(ui, '_bbfp') and
                ui._bbfp is not None):
                date = util.datestr(None, '%Y/%m/%d %H:%M:%S')
                user = util.getuser()
                pid = str(util.getpid())
                formattedmsg = msg[0] % msg[1:]
                rev = '(unknown)'
                changed = ''
                if util.safehasattr(ui, '_bbrepo'):
                    ctx = ui._bbrepo[None]
                    if ctx.rev() is not None:
                        rev = hexfn(ctx.node())
                    else:
                        parents = ctx.parents()
                        rev = ('+'.join([hexfn(p.node()) for p in parents]))
                        if (ui.configbool('blackbox', 'dirty', False) and (
                            any(ui._bbrepo.status()) or
                            any(ctx.sub(s).dirty() for s in ctx.substate)
                        )):
                            changed = '+'
                try:
                    ui._bbwrite('%s %s @%s%s (%s)> %s',
                        date, user, rev, changed, pid, formattedmsg)
                except IOError as err:
                    self.debug('warning: cannot write to blackbox.log: %s\n' %
                               err.strerror)
                if not lastui or util.safehasattr(ui, '_bbrepo'):
                    lastui = ui

        def setrepo(self, repo):
            self._bbvfs = repo.vfs
            self._bbrepo = repo

    ui.__class__ = blackboxui

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
