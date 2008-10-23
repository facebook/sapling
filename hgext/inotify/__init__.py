# __init__.py - inotify-based status acceleration for Linux
#
# Copyright 2006, 2007, 2008 Bryan O'Sullivan <bos@serpentine.com>
# Copyright 2007, 2008 Brendan Cully <brendan@kublai.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

'''inotify-based status acceleration for Linux systems
'''

# todo: socket permissions

from mercurial.i18n import gettext as _
from mercurial import cmdutil, util
import client, errno, os, server, socket
from weakref import proxy

def serve(ui, repo, **opts):
    '''start an inotify server for this repository'''
    timeout = opts.get('timeout')
    if timeout:
        timeout = float(timeout) * 1e3

    class service:
        def init(self):
            try:
                self.master = server.Master(ui, repo, timeout)
            except server.AlreadyStartedException, inst:
                raise util.Abort(str(inst))

        def run(self):
            try:
                self.master.run()
            finally:
                self.master.shutdown()

    service = service()
    cmdutil.service(opts, initfn=service.init, runfn=service.run)

def reposetup(ui, repo):
    if not repo.local():
        return

    # XXX: weakref until hg stops relying on __del__
    repo = proxy(repo)

    class inotifydirstate(repo.dirstate.__class__):
        # Set to True if we're the inotify server, so we don't attempt
        # to recurse.
        inotifyserver = False

        def status(self, match, ignored, clean, unknown=True):
            files = match.files()
            try:
                if not ignored and not self.inotifyserver:
                    result = client.query(ui, repo, files, match, False,
                                          clean, unknown)
                    if ui.config('inotify', 'debug'):
                        r2 = super(inotifydirstate, self).status(
                            match, False, clean, unknown)
                        for c,a,b in zip('LMARDUIC', result, r2):
                            for f in a:
                                if f not in b:
                                    ui.warn('*** inotify: %s +%s\n' % (c, f))
                            for f in b:
                                if f not in a:
                                    ui.warn('*** inotify: %S -%s\n' % (c, f))
                        result = r2

                    if result is not None:
                        return result
            except (OSError, socket.error), err:
                if err[0] == errno.ECONNREFUSED:
                    ui.warn(_('(found dead inotify server socket; '
                                   'removing it)\n'))
                    os.unlink(repo.join('inotify.sock'))
                if err[0] in (errno.ECONNREFUSED, errno.ENOENT) and \
                        ui.configbool('inotify', 'autostart', True):
                    query = None
                    ui.debug(_('(starting inotify server)\n'))
                    try:
                        server.start(ui, repo)
                        query = client.query
                    except server.AlreadyStartedException, inst:
                        # another process may have started its own
                        # inotify server while this one was starting.
                        ui.debug(str(inst))
                        query = client.query
                    except Exception, inst:
                        ui.warn(_('could not start inotify server: '
                                       '%s\n') % inst)
                    if query:
                        try:
                            return query(ui, repo, files or [], match,
                                         ignored, clean, unknown)
                        except socket.error, err:
                            ui.warn(_('could not talk to new inotify '
                                           'server: %s\n') % err[-1])
                elif err[0] == errno.ENOENT:
                    ui.warn(_('(inotify server not running)\n'))
                else:
                    ui.warn(_('failed to contact inotify server: %s\n')
                             % err[-1])
                ui.print_exc()
                # replace by old status function
                self.status = super(inotifydirstate, self).status

            return super(inotifydirstate, self).status(
                match, ignored, clean, unknown)

    repo.dirstate.__class__ = inotifydirstate

cmdtable = {
    '^inserve':
    (serve,
     [('d', 'daemon', None, _('run server in background')),
      ('', 'daemon-pipefds', '', _('used internally by daemon mode')),
      ('t', 'idle-timeout', '', _('minutes to sit idle before exiting')),
      ('', 'pid-file', '', _('name of file to write process ID to'))],
     _('hg inserve [OPT]...')),
    }
