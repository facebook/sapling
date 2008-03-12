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
            self.master = server.Master(ui, repo, timeout)

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

        def status(self, files, match, list_ignored, list_clean,
                   list_unknown=True):
            try:
                if not list_ignored and not self.inotifyserver:
                    result = client.query(ui, repo, files, match, False,
                                          list_clean, list_unknown)
                    if result is not None:
                        return result
            except socket.error, err:
                if err[0] == errno.ECONNREFUSED:
                    ui.warn(_('(found dead inotify server socket; '
                                   'removing it)\n'))
                    os.unlink(repo.join('inotify.sock'))
                elif err[0] != errno.ENOENT:
                    raise
                if ui.configbool('inotify', 'autostart'):
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
                        ui.print_exc()

                    if query:
                        try:
                            return query(ui, repo, files or [], match,
                                         list_ignored, list_clean, list_unknown)
                        except socket.error, err:
                            ui.warn(_('could not talk to new inotify '
                                           'server: %s\n') % err[1])
                            ui.print_exc()

            return super(inotifydirstate, self).status(
                files, match or util.always, list_ignored, list_clean,
                list_unknown)

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
