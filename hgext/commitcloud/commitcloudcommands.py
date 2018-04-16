# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import errno
import itertools
import re
import socket
import time

# Mercurial
from mercurial.i18n import _
from mercurial import (
    commands,
    discovery,
    error,
    hg,
    lock as lockmod,
    node,
    obsolete,
    obsutil,
    registrar,
)

from .. import shareutil

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
infinitepush = None

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
def cloudleave(ui, repo, **opts):
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
def cloudsync(ui, repo, dest=None, **opts):
    """synchronize commits with the commit cloud service"""

    try:
        # Wait at most 30 seconds, because that's the average backup time
        timeout = 30
        srcrepo = shareutil.getsrcrepo(repo)
        with lockmod.lock(srcrepo.vfs,
                          infinitepush.backupcommands._backuplockname,
                          timeout=timeout):
            currentnode = repo['.'].node()
            _docloudsync(ui, repo, dest, **opts)
            return _maybeupdateworkingcopy(ui, repo, currentnode)
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            ui.warn(_('timeout waiting on backup lock\n'))
            return 0
        else:
            raise

def _docloudsync(ui, repo, dest=None, **opts):
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

            # First, push commits that the server doesn't have.
            newheads = list(set(localheads) - set(lastsyncstate.heads))
            pushbackup(ui, repo, newheads, localheads, localbookmarks, dest,
                       **opts)

            # Next, update the cloud heads, bookmarks and obsmarkers.
            obsmarkers = []
            if repo.svfs.exists('commitcloudpendingobsmarkers'):
                with repo.svfs.open('commitcloudpendingobsmarkers') as f:
                    _version, obsmarkers = obsolete._readmarkers(f.read())
            synced, cloudrefs = serv.updatereferences(
                lastsyncstate.version, lastsyncstate.heads, localheads,
                lastsyncstate.bookmarks.keys(), localbookmarks, obsmarkers)
            if synced:
                lastsyncstate.update(cloudrefs.version, localheads,
                                     localbookmarks)
                if obsmarkers:
                    repo.svfs.unlink('commitcloudpendingobsmarkers')

    elapsed = time.time() - start
    highlightdebug(ui, _('cloudsync is done in %0.2f sec\n') % elapsed)
    highlightstatus(ui, _('cloudsync done\n'))

def _maybeupdateworkingcopy(ui, repo, currentnode):
    if repo['.'].node() != currentnode:
        return 0

    destination = finddestinationnode(repo, currentnode)

    if destination == currentnode:
        return 0

    if destination and destination in repo:
        highlightstatus(
            ui, _(
                'current revision %s has been moved remotely to %s\n') %
            (node.short(currentnode), node.short(
                destination)))
        if ui.configbool('commitcloud', 'updateonmove'):
            return _update(ui, repo, destination)
    else:
        highlightstatus(
            ui,
            _('current revision %s has been replaced remotely '
                'with multiple revisions\n'
              'Please run `hg update` to go to the desired revision\n') %
            node.short(currentnode))
        return 0

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

    # Also update infinitepush state.  These new heads are already backed up,
    # otherwise the server wouldn't have told us about them.
    if newheads:
        recordbackup(ui, repo, newheads)

def _update(ui, repo, destination):
    # update to new head with merging local uncommited changes
    ui.status(_('updating to %s\n') % node.short(destination))
    updatecheck = 'none'
    return hg.updatetotally(ui, repo, destination, destination,
                            updatecheck=updatecheck)

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

def getsuccessorsnodes(repo, node):
    successors = repo.obsstore.successors.get(node, ())
    for successor in successors:
        m = obsutil.marker(repo, successor)
        for snode in m.succnodes():
            if snode and snode != node:
                yield snode

def finddestinationnode(repo, node):
    nodes = list(getsuccessorsnodes(repo, node))
    if len(nodes) == 1:
        return finddestinationnode(repo, nodes[0])
    if len(nodes) == 0:
        return node
    return None

def pushbackup(ui, repo, newheads, localheads, localbookmarks, dest, **opts):
    """Push a backup bundle to the server containing the new heads."""
    # Calculate the commits to back-up.  The bundle needs to cleanly apply
    # to the server, so we need to include the whole draft stack.
    commitstobackup = repo.set('draft() & ::%ln',
                               [node.bin(h) for h in newheads])

    # Calculate the parent commits of the commits we are backing up.  These
    # are the public commits that should be on the server.
    parentcommits = repo.set('parents(roots(%ln))', commitstobackup)

    # Build a discovery object encapsulating the commits to backup.
    # Skip the actual discovery process, as we know exactly which
    # commits are missing.  For common commits, include all the
    # parents of the commits we are sending.
    og = discovery.outgoing(repo, commonheads=parentcommits,
                            missingheads=newheads)
    og._missing = [c.node() for c in commitstobackup]
    og._common = [c.node() for c in parentcommits]

    # Build a dictionary of infinitepush bookmarks.  We delete
    # all bookmarks and replace them with the full set each time.
    namingmgr = infinitepush.backupcommands.BackupBookmarkNamingManager(
            ui, repo, opts.get('user'))
    infinitepushbookmarks = {}
    infinitepushbookmarks[namingmgr.getbackupheadprefix()] = ''
    infinitepushbookmarks[namingmgr.getbackupbookmarkprefix()] = ''
    for bookmark, hexnode in localbookmarks.items():
        name = namingmgr.getbackupbookmarkname(bookmark)
        infinitepushbookmarks[name] = hexnode
    for hexhead in localheads:
        name = namingmgr.getbackupheadname(hexhead)
        infinitepushbookmarks[name] = hexhead

    # Push these commits to the server.
    other = infinitepush.backupcommands._getremote(repo, ui, dest, **opts)
    infinitepush.backupcommands._dobackuppush(ui, repo, other, og,
                                              infinitepushbookmarks)

    # Update the infinitepush local state.
    infinitepush.backupcommands._writelocalbackupstate(
            repo.vfs, list(localheads), localbookmarks)

def recordbackup(ui, repo, newheads):
    """Record that the given heads are already backed up."""
    backupstate = infinitepush.backupcommands._readlocalbackupstate(ui, repo)
    backupheads = set(backupstate.heads) | set(newheads)
    infinitepush.backupcommands._writelocalbackupstate(
            repo.vfs, list(backupheads), backupstate.localbookmarks)
