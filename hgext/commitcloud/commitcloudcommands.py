# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import itertools
import re
import socket
import time

# Mercurial
from mercurial.i18n import _
from mercurial import (
    commands,
    node,
    obsolete,
    registrar,
)

from . import (
    commitcloudcommon,
    commitcloudutil,
    service,
    state,
)

cmdtable = {}
command = registrar.command(cmdtable)
highlightdebug = commitcloudcommon.highlightdebug
highlightstatus = commitcloudcommon.highlightstatus

@command('cloudjoin')
def cloudjoin(ui, repo, **opts):
    """joins the local repository to a cloud workspace

    This will keep all commits, bookmarks, and working copy parents
    the same across all the repositories that are part of the same workspace.

    For instance, a common use case is keeping laptop and desktop repos in sync.

    Currently only a single default workspace for the user is supported.
    """

    if not commitcloudutil.TokenLocator(ui).token:
        raise commitcloudcommon.RegistrationError(
            ui, _('please run `hg cloudregister` before joining a workspace'))

    workspacemanager = commitcloudutil.WorkspaceManager(repo)
    workspacemanager.setworkspace()

    highlightstatus(
        ui, _("this repository is now part of the '%s' "
              "workspace for the '%s' repo\n") %
        (workspacemanager.workspace, workspacemanager.reponame))

@command('cloudleave')
def uncloudjoin(ui, repo, **opts):
    """leave Commit Cloud synchronization

    The command disconnect this local repo from any of Commit Cloud workspaces
    """
    commitcloudutil.WorkspaceManager(repo).clearworkspace()
    highlightstatus(ui, _('you are no longer connected to a workspace\n'))

@command('cloudregister', [('t', 'token', '', 'set secret access token')])
def cloudregister(ui, repo, **opts):
    """register your private access token with Commit Cloud for this host

    This can be done in any hg repo with Commit Cloud enabled on the host
    """
    tokenlocator = commitcloudutil.TokenLocator(ui)
    highlightstatus(ui, _('welcome to registration!\n'))

    token = opts.get('token')
    if not token:
        token = tokenlocator.token
        if not token:
            msg = _('token is not provided and not found')
            raise commitcloudcommon.RegistrationError(ui, msg)
        else:
            ui.status(_('you have been already registered\n'))
            return
    else:
        if tokenlocator.token:
            ui.status(_('your token will be updated\n'))
        tokenlocator.settoken(token)
    ui.status(_('registration successful\n'))

@command('cloudsync')
def cloudsync(ui, repo, **opts):
    """synchronize commits with the commit cloud service"""

    start = time.time()
    serv = service.get(ui, repo)

    lastsyncstate = state.SyncState(repo)
    cloudrefs = serv.getreferences(lastsyncstate.version)
    highlightstatus(ui, 'start synchronization\n')

    synced = False
    while not synced:
        if cloudrefs.version != lastsyncstate.version:
            _applycloudchanges(ui, repo, lastsyncstate, cloudrefs)

        localheads = _getheads(repo)
        localbookmarks = _getbookmarks(repo)

        if (set(localheads) == set(lastsyncstate.heads) and
                localbookmarks == lastsyncstate.bookmarks and
                lastsyncstate.version != 0):
            synced = True

        if not synced:
            # The local repo has changed.  We must send these changes to the
            # cloud.
            obsmarkers = []
            if repo.svfs.exists('commitcloudpendingobsmarkers'):
                with repo.svfs.open('commitcloudpendingobsmarkers') as f:
                    _version, obsmarkers = obsolete._readmarkers(f.read())
            synced, cloudrefs = serv.updatereferences(
                lastsyncstate.version, lastsyncstate.heads, localheads,
                lastsyncstate.bookmarks, localbookmarks, obsmarkers)
            if synced:
                lastsyncstate.update(cloudrefs.version, localheads,
                                     localbookmarks)
                if obsmarkers:
                    repo.svfs.unlink('commitcloudpendingobsmarkers')

    elapsed = time.time() - start
    highlightdebug(ui, _('cloudsync is done in %0.2f sec\n') % elapsed)
    highlightstatus(ui, _('cloudsync done\n'))

@command('cloudrecover')
def cloudrecover(ui, repo, **opts):
    """recover Commit Cloud State

    It cleans up Commit Cloud internal files in the repo
    and synchronize it from scratch
    """
    highlightstatus(ui, 'start recovering\n')
    state.SyncState.erasestate(repo)
    cloudsync(ui, repo, **opts)

def _applycloudchanges(ui, repo, lastsyncstate, cloudrefs):
    pullcmd, pullopts = _getcommandandoptions('^pull')

    # Pull all the new heads
    # so we need to filter cloudrefs before pull
    # pull does't check if a rev is present locally
    unfi = repo.unfiltered()
    newheads = filter(lambda rev: rev not in unfi, cloudrefs.heads)
    if newheads:
        pullopts['rev'] = newheads
        pullcmd(ui, repo, **pullopts)

    # Merge cloud bookmarks into the repo
    _mergebookmarks(ui, repo, cloudrefs.bookmarks, lastsyncstate.bookmarks)

    # Merge obsmarkers
    _mergeobsmarkers(ui, repo, cloudrefs.obsmarkers)

    # We have now synced the repo to the cloud version.  Store this.
    lastsyncstate.update(cloudrefs.version, cloudrefs.heads,
                         cloudrefs.bookmarks)

def _mergebookmarks(ui, repo, cloudbookmarks, lastsyncbookmarks):
    localbookmarks = _getbookmarks(repo)
    with repo.wlock(), repo.lock(), repo.transaction('bookmark') as tr:
        changes = []
        allnames = set(localbookmarks.keys() + cloudbookmarks.keys())
        newnames = set()
        for name in allnames:
            localnode = localbookmarks.get(name)
            cloudnode = cloudbookmarks.get(name)
            lastnode = lastsyncbookmarks.get(name)
            if cloudnode != localnode:
                if (localnode is not None and cloudnode is not None and
                        localnode != lastnode and cloudnode != lastnode):
                    # Changed both locally and remotely, fork the local
                    # bookmark
                    forkname = _forkname(ui, name, allnames | newnames)
                    newnames.add(forkname)
                    changes.append((forkname, node.bin(localnode)))
                    ui.warn(_('%s changed locally and remotely, '
                              'local bookmark renamed to %s\n') %
                            (name, forkname))

                if cloudnode != lastnode:
                    if cloudnode is not None:
                        if cloudnode in repo:
                            changes.append((name, node.bin(cloudnode)))
                        else:
                            ui.warn(_('%s not found, '
                                      'not creating %s bookmark\n') %
                                    (cloudnode, name))
                    else:
                        if localnode is not None and localnode != lastnode:
                            # Moved locally, deleted in the cloud, resurrect
                            # at the new location
                            pass
                        else:
                            changes.append((name, None))
        repo._bookmarks.applychanges(repo, tr, changes)

def _mergeobsmarkers(ui, repo, obsmarkers):
    with repo.wlock(), repo.lock(), repo.transaction('commitcloud-obs') as tr:
        tr._commitcloudskippendingobsmarkers = True
        repo.obsstore.add(tr, obsmarkers)

def _forkname(ui, name, othernames):
    hostname = ui.config('commitcloud', 'hostname', socket.gethostname())

    # Strip off any old suffix.
    m = re.match('-%s(-[0-9]*)?$' % re.escape(hostname), name)
    if m:
        suffix = '-%s%s' % (hostname, m.group(1) or '')
        name = name[0:-len(suffix)]

    # Find a new name.
    for n in itertools.count():
        candidate = '%s-%s%s' % (name, hostname, '-%s' % n if n != 0 else '')
        if candidate not in othernames:
            return candidate

def _getheads(repo):
    headsrevset = repo.set('heads(draft()) & not obsolete()')
    return [ctx.hex() for ctx in headsrevset]

def _getbookmarks(repo):
    return {n: node.hex(v) for n, v in repo._bookmarks.items()}

def _getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts
