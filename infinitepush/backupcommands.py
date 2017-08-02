# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
    [infinitepushbackup]
    # path to the directory where pushback logs should be stored
    logdir = path/to/dir

    # Backup at most maxheadstobackup heads, other heads are ignored.
    # Negative number means backup everything.
    maxheadstobackup = -1

    # Nodes that should not be backed up. Ancestors of these nodes won't be
    # backed up either
    dontbackupnodes = []

    # Special option that may be used to trigger re-backuping. For example,
    # if there was a bug in infinitepush backups, then changing the value of
    # this option will force all clients to make a "clean" backup
    backupgeneration = 0

    # Hostname value to use. If not specified then socket.gethostname() will
    # be used
    hostname = ''

    # Enable reporting of infinitepush backup status as a summary at the end
    # of smartlog.
    enablestatus = False

    # Whether or not to save information about the latest successful backup.
    # This information includes the local revision number and unix timestamp
    # of the last time we successfully made a backup.
    savelatestbackupinfo = False
"""

from __future__ import absolute_import
import errno
import json
import os
import re
import socket
import stat
import subprocess
import time

from .bundleparts import (
    getscratchbookmarkspart,
    getscratchbranchpart,
)
from mercurial import (
    bundle2,
    changegroup,
    commands,
    discovery,
    encoding,
    error,
    hg,
    lock as lockmod,
    phases,
    registrar,
    scmutil,
    util,
)

from collections import defaultdict, namedtuple
from mercurial import policy
from mercurial.extensions import wrapfunction, unwrapfunction
from mercurial.node import bin, hex, nullrev, short
from mercurial.i18n import _

osutil = policy.importmod(r'osutil')

cmdtable = {}
command = registrar.command(cmdtable)
revsetpredicate = registrar.revsetpredicate()

backupbookmarktuple = namedtuple('backupbookmarktuple',
                                 ['hostname', 'reporoot', 'localbookmark'])

class backupstate(object):
    def __init__(self):
        self.heads = set()
        self.localbookmarks = {}

    def empty(self):
        return not self.heads and not self.localbookmarks

class WrongPermissionsException(Exception):
    def __init__(self, logdir):
        self.logdir = logdir

restoreoptions = [
     ('', 'reporoot', '', 'root of the repo to restore'),
     ('', 'user', '', 'user who ran the backup'),
     ('', 'hostname', '', 'hostname of the repo to restore'),
]

_backuplockname = 'infinitepushbackup.lock'

@command('pushbackup',
         [('', 'background', None, 'run backup in background')])
def backup(ui, repo, dest=None, **opts):
    """
    Pushes commits, bookmarks and heads to infinitepush.
    New non-extinct commits are saved since the last `hg pushbackup`
    or since 0 revision if this backup is the first.
    Local bookmarks are saved remotely as:
        infinitepush/backups/USERNAME/HOST/REPOROOT/bookmarks/LOCAL_BOOKMARK
    Local heads are saved remotely as:
        infinitepush/backups/USERNAME/HOST/REPOROOT/heads/HEAD_HASH
    """

    if opts.get('background'):
        background_cmd = ['hg', 'pushbackup']
        if dest:
            background_cmd.append(dest)
        logfile = None
        logdir = ui.config('infinitepushbackup', 'logdir')
        if logdir:
            # make newly created files and dirs non-writable
            oldumask = os.umask(0o022)
            try:
                try:
                    username = util.shortuser(ui.username())
                except Exception:
                    username = 'unknown'

                if not _checkcommonlogdir(logdir):
                    raise WrongPermissionsException(logdir)

                userlogdir = os.path.join(logdir, username)
                util.makedirs(userlogdir)

                if not _checkuserlogdir(userlogdir):
                    raise WrongPermissionsException(userlogdir)

                reporoot = repo.origroot
                reponame = os.path.basename(reporoot)
                _removeoldlogfiles(userlogdir, reponame)
                logfile = _getlogfilename(logdir, username, reponame)
            except (OSError, IOError) as e:
                ui.debug('infinitepush backup log is disabled: %s\n' % e)
            except WrongPermissionsException as e:
                ui.debug(('%s directory has incorrect permission, ' +
                         'infinitepush backup logging will be disabled\n') %
                         e.logdir)
            finally:
                os.umask(oldumask)

        if not logfile:
            logfile = os.devnull

        with open(logfile, 'a') as f:
            subprocess.Popen(background_cmd, shell=False, stdout=f,
                             stderr=subprocess.STDOUT)
        return 0

    try:
        # Wait at most 30 seconds, because that's the average backup time
        timeout = 30
        srcrepo = _getsrcrepo(repo)
        with lockmod.lock(srcrepo.vfs, _backuplockname, timeout=timeout):
            return _dobackup(ui, repo, dest, **opts)
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            ui.warn(_('timeout waiting on backup lock\n'))
            return 0
        else:
            raise

@command('pullbackup', restoreoptions)
def restore(ui, repo, dest=None, **opts):
    """
    Pulls commits from infinitepush that were previously saved with
    `hg pushbackup`.
    If user has only one backup for the `dest` repo then it will be restored.
    But user may have backed up many local repos that points to `dest` repo.
    These local repos may reside on different hosts or in different
    repo roots. It makes restore ambiguous; `--reporoot` and `--hostname`
    options are used to disambiguate.
    """

    other = _getremote(repo, ui, dest, **opts)

    sourcereporoot = opts.get('reporoot')
    sourcehostname = opts.get('hostname')
    namingmgr = BackupBookmarkNamingManager(ui, repo, opts.get('user'))
    allbackupstates = _downloadbackupstate(ui, other, sourcereporoot,
                                           sourcehostname, namingmgr)
    if len(allbackupstates) == 0:
        ui.warn(_('no backups found!'))
        return 1
    _checkbackupstates(allbackupstates)

    __, backupstate = allbackupstates.popitem()
    pullcmd, pullopts = _getcommandandoptions('^pull')
    # pull backuped heads and nodes that are pointed by bookmarks
    pullopts['rev'] = list(backupstate.heads |
                           set(backupstate.localbookmarks.values()))
    if dest:
        pullopts['source'] = dest
    result = pullcmd(ui, repo, **pullopts)

    with repo.wlock(), repo.lock(), repo.transaction('bookmark') as tr:
        changes = []
        for book, hexnode in backupstate.localbookmarks.iteritems():
            if hexnode in repo:
                changes.append((book, bin(hexnode)))
            else:
                ui.warn(_('%s not found, not creating %s bookmark') %
                        (hexnode, book))
        repo._bookmarks.applychanges(repo, tr, changes)

    return result

@command('getavailablebackups',
    [('', 'user', '', _('username, defaults to current user')),
     ('', 'json', None, _('print available backups in json format'))])
def getavailablebackups(ui, repo, dest=None, **opts):
    other = _getremote(repo, ui, dest, **opts)

    sourcereporoot = opts.get('reporoot')
    sourcehostname = opts.get('hostname')

    namingmgr = BackupBookmarkNamingManager(ui, repo, opts.get('user'))
    allbackupstates = _downloadbackupstate(ui, other, sourcereporoot,
                                           sourcehostname, namingmgr)

    if opts.get('json'):
        jsondict = defaultdict(list)
        for hostname, reporoot in allbackupstates.keys():
            jsondict[hostname].append(reporoot)
            # make sure the output is sorted. That's not an efficient way to
            # keep list sorted but we don't have that many backups.
            jsondict[hostname].sort()
        ui.write('%s\n' % json.dumps(jsondict))
    else:
        if not allbackupstates:
            ui.write(_('no backups available for %s\n') % namingmgr.username)

        ui.write(_('user %s has %d available backups:\n') %
                 (namingmgr.username, len(allbackupstates)))

        for hostname, reporoot in sorted(allbackupstates.keys()):
            ui.write(_('%s on %s\n') % (reporoot, hostname))

@command('debugcheckbackup',
         [('', 'all', None, _('check all backups that user have')),
         ] + restoreoptions)
def checkbackup(ui, repo, dest=None, **opts):
    """
    Checks that all the nodes that backup needs are available in bundlestore
    This command can check either specific backup (see restoreoptions) or all
    backups for the user
    """

    sourcereporoot = opts.get('reporoot')
    sourcehostname = opts.get('hostname')

    other = _getremote(repo, ui, dest, **opts)
    namingmgr = BackupBookmarkNamingManager(ui, repo, opts.get('user'))
    allbackupstates = _downloadbackupstate(ui, other, sourcereporoot,
                                           sourcehostname, namingmgr)
    if not opts.get('all'):
        _checkbackupstates(allbackupstates)

    ret = 0
    while allbackupstates:
        key, bkpstate = allbackupstates.popitem()
        ui.status(_('checking %s on %s\n') % (key[1], key[0]))
        if not _dobackupcheck(bkpstate, ui, repo, dest, **opts):
            ret = 255
    return ret

@command('debugwaitbackup', [('', 'timeout', '', 'timeout value')])
def waitbackup(ui, repo, timeout):
    try:
        if timeout:
            timeout = int(timeout)
        else:
            timeout = -1
    except ValueError:
        raise error.Abort('timeout should be integer')

    try:
        repo = _getsrcrepo(repo)
        with lockmod.lock(repo.vfs, _backuplockname, timeout=timeout):
            pass
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            raise error.Abort(_('timeout while waiting for backup'))
        raise

@command('isbackedup',
     [('r', 'rev', [], _('show the specified revision or revset'), _('REV'))])
def isbackedup(ui, repo, **opts):
    """checks if commit was backed up to infinitepush

    If no revision are specified then it checks working copy parent
    """

    revs = opts.get('rev')
    if not revs:
        revs = ['.']
    bkpstate = _readlocalbackupstate(ui, repo)
    unfi = repo.unfiltered()
    backeduprevs = unfi.revs('draft() and ::%ls', bkpstate.heads)
    for r in scmutil.revrange(unfi, revs):
        ui.write(_(unfi[r].hex() + ' '))
        ui.write(_('backed up' if r in backeduprevs else 'not backed up'))
        ui.write(_('\n'))

@revsetpredicate('backedup')
def backedup(repo, subset, x):
    """Draft changesets that have been backed up by infinitepush"""
    bkpstate = _readlocalbackupstate(repo.ui, repo)
    visiblebkpheads = [head for head in bkpstate.heads if head in repo]
    return subset & repo.revs('draft() and ::%ls', visiblebkpheads)

def smartlogsummary(ui, repo):
    if not ui.configbool('infinitepushbackup', 'enablestatus'):
        return

    bkpstate = _readlocalbackupstate(ui, repo)
    visiblebkpheads = [head for head in bkpstate.heads if head in repo]
    unbackeduprevs = repo.revs('draft() and not ::%ls', visiblebkpheads)

    # Count the number of changesets that haven't been backed up for 10 minutes.
    # If there is only one, also print out its hash.
    backuptime = time.time() - 10 * 60  # 10 minutes ago
    count = 0
    singleunbackeduprev = None
    for rev in unbackeduprevs:
        if repo[rev].date()[0] <= backuptime:
            singleunbackeduprev = rev
            count += 1
    if count > 0:
        if count > 1:
            ui.warn(_('note: %d changesets are not backed up.\n') % count)
        else:
            ui.warn(_('note: changeset %s is not backed up.\n') %
                    short(repo[singleunbackeduprev].node()))
        ui.warn(_('Run `hg pushbackup` to perform a backup.  If this fails,\n'
                  'please report to the Source Control @ FB group.\n'))

def _dobackup(ui, repo, dest, **opts):
    ui.status(_('starting backup %s\n') % time.strftime('%H:%M:%S %d %b %Y %Z'))
    start = time.time()
    # to handle multiple working copies correctly
    repo = _getsrcrepo(repo)
    currentbkpgenerationvalue = _readbackupgenerationfile(repo.vfs)
    newbkpgenerationvalue = ui.configint('infinitepushbackup',
                                         'backupgeneration', 0)
    if currentbkpgenerationvalue != newbkpgenerationvalue:
        # Unlinking local backup state will trigger re-backuping
        _deletebackupstate(repo)
        _writebackupgenerationfile(repo.vfs, newbkpgenerationvalue)
    bkpstate = _readlocalbackupstate(ui, repo)

    maxheadstobackup = ui.configint('infinitepushbackup',
                                    'maxheadstobackup', -1)

    revset = 'head() & draft() & not obsolete()'

    # this variable stores the local store info (tip numeric revision and date)
    # which we use to quickly tell if our backup is stale
    afterbackupinfo = _getlocalinfo(repo)

    # This variable will store what heads will be saved in backup state file
    # if backup finishes successfully
    afterbackupheads = [ctx.hex() for ctx in repo.set(revset)]
    if maxheadstobackup > 0:
        afterbackupheads = afterbackupheads[-maxheadstobackup:]
    elif maxheadstobackup == 0:
        afterbackupheads = []
    afterbackupheads = set(afterbackupheads)
    other = _getremote(repo, ui, dest, **opts)
    outgoing, badhexnodes = _getrevstobackup(repo, ui, other,
                                             afterbackupheads - bkpstate.heads)
    # If remotefilelog extension is enabled then there can be nodes that we
    # can't backup. In this case let's remove them from afterbackupheads
    afterbackupheads.difference_update(badhexnodes)

    # As afterbackupheads this variable stores what heads will be saved in
    # backup state file if backup finishes successfully
    afterbackuplocalbooks = _getlocalbookmarks(repo)
    afterbackuplocalbooks = _filterbookmarks(
        afterbackuplocalbooks, repo, afterbackupheads)

    newheads = afterbackupheads - bkpstate.heads
    removedheads = bkpstate.heads - afterbackupheads
    newbookmarks = _dictdiff(afterbackuplocalbooks, bkpstate.localbookmarks)
    removedbookmarks = _dictdiff(bkpstate.localbookmarks, afterbackuplocalbooks)

    namingmgr = BackupBookmarkNamingManager(ui, repo)
    bookmarkstobackup = _getbookmarkstobackup(
        repo, newbookmarks, removedbookmarks,
        newheads, removedheads, namingmgr)

    # Special case if backup state is empty. Clean all backup bookmarks from the
    # server.
    if bkpstate.empty():
        bookmarkstobackup[namingmgr.getbackupheadprefix()] = ''
        bookmarkstobackup[namingmgr.getbackupbookmarkprefix()] = ''

    # Wrap deltaparent function to make sure that bundle takes less space
    # See _deltaparent comments for details
    wrapfunction(changegroup.cg2packer, 'deltaparent', _deltaparent)
    try:
        bundler = _createbundler(ui, repo, other)
        backup = False
        if outgoing and outgoing.missing:
            backup = True
            bundler.addpart(getscratchbranchpart(repo, other, outgoing,
                                                 confignonforwardmove=False,
                                                 ui=ui, bookmark=None,
                                                 create=False))

        if bookmarkstobackup:
            backup = True
            bundler.addpart(getscratchbookmarkspart(other, bookmarkstobackup))

        if backup:
            _sendbundle(bundler, other)
            _writelocalbackupstate(repo.vfs, afterbackupheads,
                                   afterbackuplocalbooks)
            if ui.config('infinitepushbackup', 'savelatestbackupinfo'):
                _writelocalbackupinfo(repo.vfs, **afterbackupinfo)
        else:
            ui.status(_('nothing to backup\n'))
    finally:
        # cleanup ensures that all pipes are flushed
        try:
            other.cleanup()
        except Exception:
            ui.warn(_('remote connection cleanup failed\n'))
        ui.status(_('finished in %f seconds\n') % (time.time() - start))
        unwrapfunction(changegroup.cg2packer, 'deltaparent', _deltaparent)
    return 0

def _dobackupcheck(bkpstate, ui, repo, dest, **opts):
    remotehexnodes = list(bkpstate.heads) + bkpstate.localbookmarks.values()
    if not remotehexnodes:
        return True
    other = _getremote(repo, ui, dest, **opts)
    batch = other.iterbatch()
    for hexnode in remotehexnodes:
        batch.lookup(hexnode)
    batch.submit()
    lookupresults = batch.results()
    try:
        for r in lookupresults:
            # iterate over results to make it throw if revision
            # was not found
            pass
        return True
    except error.RepoError as e:
        ui.warn(_('%s\n') % e)
        return False

_backuplatestinfofile = 'infinitepushlatestbackupinfo'
_backupstatefile = 'infinitepushbackupstate'
_backupgenerationfile = 'infinitepushbackupgeneration'

# Common helper functions
def _getlocalinfo(repo):
    localinfo = {}
    localinfo['rev'] = repo[repo.changelog.tip()].rev()
    localinfo['time'] = int(time.time())
    return localinfo

def _getlocalbookmarks(repo):
    localbookmarks = {}
    for bookmark, node in repo._bookmarks.iteritems():
        hexnode = hex(node)
        localbookmarks[bookmark] = hexnode
    return localbookmarks

def _filterbookmarks(localbookmarks, repo, headstobackup):
    '''Filters out some bookmarks from being backed up

    Filters out bookmarks that do not point to ancestors of headstobackup or
    public commits
    '''

    headrevstobackup = [repo[hexhead].rev() for hexhead in headstobackup]
    ancestors = repo.changelog.ancestors(headrevstobackup, inclusive=True)
    filteredbooks = {}
    for bookmark, hexnode in localbookmarks.iteritems():
        if (repo[hexnode].rev() in ancestors or
                repo[hexnode].phase() == phases.public):
            filteredbooks[bookmark] = hexnode
    return filteredbooks

def _downloadbackupstate(ui, other, sourcereporoot, sourcehostname, namingmgr):
    pattern = namingmgr.getcommonuserprefix()
    fetchedbookmarks = other.listkeyspatterns('bookmarks', patterns=[pattern])
    allbackupstates = defaultdict(backupstate)
    for book, hexnode in fetchedbookmarks.iteritems():
        parsed = _parsebackupbookmark(book, namingmgr)
        if parsed:
            if sourcereporoot and sourcereporoot != parsed.reporoot:
                continue
            if sourcehostname and sourcehostname != parsed.hostname:
                continue
            key = (parsed.hostname, parsed.reporoot)
            if parsed.localbookmark:
                bookname = parsed.localbookmark
                allbackupstates[key].localbookmarks[bookname] = hexnode
            else:
                allbackupstates[key].heads.add(hexnode)
        else:
            ui.warn(_('wrong format of backup bookmark: %s') % book)

    return allbackupstates

def _checkbackupstates(allbackupstates):
    if len(allbackupstates) == 0:
        raise error.Abort('no backups found!')

    hostnames = set(key[0] for key in allbackupstates.iterkeys())
    reporoots = set(key[1] for key in allbackupstates.iterkeys())

    if len(hostnames) > 1:
        raise error.Abort(
            _('ambiguous hostname to restore: %s') % sorted(hostnames),
            hint=_('set --hostname to disambiguate'))

    if len(reporoots) > 1:
        raise error.Abort(
            _('ambiguous repo root to restore: %s') % sorted(reporoots),
            hint=_('set --reporoot to disambiguate'))

class BackupBookmarkNamingManager(object):
    def __init__(self, ui, repo, username=None):
        self.ui = ui
        self.repo = repo
        if not username:
            username = util.shortuser(ui.username())
        self.username = username

        self.hostname = self.ui.config('infinitepushbackup', 'hostname')
        if not self.hostname:
            self.hostname = socket.gethostname()

    def getcommonuserprefix(self):
        return '/'.join((self._getcommonuserprefix(), '*'))

    def getcommonprefix(self):
        return '/'.join((self._getcommonprefix(), '*'))

    def getbackupbookmarkprefix(self):
        return '/'.join((self._getbackupbookmarkprefix(), '*'))

    def getbackupbookmarkname(self, bookmark):
        bookmark = _escapebookmark(bookmark)
        return '/'.join((self._getbackupbookmarkprefix(), bookmark))

    def getbackupheadprefix(self):
        return '/'.join((self._getbackupheadprefix(), '*'))

    def getbackupheadname(self, hexhead):
        return '/'.join((self._getbackupheadprefix(), hexhead))

    def _getbackupbookmarkprefix(self):
        return '/'.join((self._getcommonprefix(), 'bookmarks'))

    def _getbackupheadprefix(self):
        return '/'.join((self._getcommonprefix(), 'heads'))

    def _getcommonuserprefix(self):
        return '/'.join(('infinitepush', 'backups', self.username))

    def _getcommonprefix(self):
        reporoot = self.repo.origroot

        result = '/'.join((self._getcommonuserprefix(), self.hostname))
        if not reporoot.startswith('/'):
            result += '/'
        result += reporoot
        if result.endswith('/'):
            result = result[:-1]
        return result

def _escapebookmark(bookmark):
    '''
    If `bookmark` contains "bookmarks" as a substring then replace it with
    "bookmarksbookmarks". This will make parsing remote bookmark name
    unambigious.
    '''

    bookmark = encoding.fromlocal(bookmark)
    return bookmark.replace('bookmarks', 'bookmarksbookmarks')

def _unescapebookmark(bookmark):
    bookmark = encoding.tolocal(bookmark)
    return bookmark.replace('bookmarksbookmarks', 'bookmarks')

def _getremote(repo, ui, dest, **opts):
    path = ui.paths.getpath(dest, default=('infinitepush', 'default'))
    if not path:
        raise error.Abort(_('default repository not configured!'),
                         hint=_("see 'hg help config.paths'"))
    dest = path.pushloc or path.loc
    return hg.peer(repo, opts, dest)

def _getcommandandoptions(command):
    cmd = commands.table[command][0]
    opts = dict(opt[1:3] for opt in commands.table[command][1])
    return cmd, opts

# Backup helper functions

def _deltaparent(orig, self, revlog, rev, p1, p2, prev):
    # This version of deltaparent prefers p1 over prev to use less space
    dp = revlog.deltaparent(rev)
    if dp == nullrev and not revlog.storedeltachains:
        # send full snapshot only if revlog configured to do so
        return nullrev
    return p1

def _getbookmarkstobackup(repo, newbookmarks, removedbookmarks,
                          newheads, removedheads, namingmgr):
    bookmarkstobackup = {}

    for bookmark, hexnode in removedbookmarks.items():
        backupbookmark = namingmgr.getbackupbookmarkname(bookmark)
        bookmarkstobackup[backupbookmark] = ''

    for bookmark, hexnode in newbookmarks.items():
        backupbookmark = namingmgr.getbackupbookmarkname(bookmark)
        bookmarkstobackup[backupbookmark] = hexnode

    for hexhead in removedheads:
        headbookmarksname = namingmgr.getbackupheadname(hexhead)
        bookmarkstobackup[headbookmarksname] = ''

    for hexhead in newheads:
        headbookmarksname = namingmgr.getbackupheadname(hexhead)
        bookmarkstobackup[headbookmarksname] = hexhead

    return bookmarkstobackup

def _createbundler(ui, repo, other):
    bundler = bundle2.bundle20(ui, bundle2.bundle2caps(other))
    # Disallow pushback because we want to avoid taking repo locks.
    # And we don't need pushback anyway
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo,
                                                      allowpushback=False))
    bundler.newpart('replycaps', data=capsblob)
    return bundler

def _sendbundle(bundler, other):
    stream = util.chunkbuffer(bundler.getchunks())
    try:
        other.unbundle(stream, ['force'], other.url())
    except error.BundleValueError as exc:
        raise error.Abort(_('missing support for %s') % exc)

def findcommonoutgoing(repo, other, heads):
    if heads:
        nodes = map(repo.changelog.node, heads)
        return discovery.findcommonoutgoing(repo, other, onlyheads=nodes)
    else:
        return None

def _getrevstobackup(repo, ui, other, headstobackup):
    # In rare cases it's possible to have a local node without filelogs.
    # This is possible if remotefilelog is enabled and if the node was
    # stripped server-side. We want to filter out these bad nodes and all
    # of their descendants.
    badnodes = ui.configlist('infinitepushbackup', 'dontbackupnodes', [])
    badnodes = [node for node in badnodes if node in repo]
    badrevs = [repo[node].rev() for node in badnodes]
    badnodesdescendants = repo.set('%ld::', badrevs) if badrevs else set()
    badnodesdescendants = set(ctx.hex() for ctx in badnodesdescendants)
    filteredheads = filter(lambda head: head in badnodesdescendants,
                           headstobackup)

    if filteredheads:
        ui.warn(_('filtering nodes: %s\n') % filteredheads)
        ui.log('infinitepushbackup', 'corrupted nodes found',
               infinitepushbackupcorruptednodes='failure')
    headstobackup = filter(lambda head: head not in badnodesdescendants,
                           headstobackup)

    revs = list(repo[hexnode].rev() for hexnode in headstobackup)
    outgoing = findcommonoutgoing(repo, other, revs)
    nodeslimit = 1000
    if outgoing and len(outgoing.missing) > nodeslimit:
        # trying to push too many nodes usually means that there is a bug
        # somewhere. Let's be safe and avoid pushing too many nodes at once
        raise error.Abort('trying to back up too many nodes: %d' %
                          (len(outgoing.missing),))
    return outgoing, set(filteredheads)

def _localbackupstateexists(repo):
    return repo.vfs.exists(_backupstatefile)

def _deletebackupstate(repo):
    return repo.vfs.tryunlink(_backupstatefile)

def _readlocalbackupstate(ui, repo):
    if not _localbackupstateexists(repo):
        return backupstate()

    with repo.vfs(_backupstatefile) as f:
        try:
            state = json.loads(f.read())
            if (not isinstance(state['bookmarks'], dict) or
                    not isinstance(state['heads'], list)):
                raise ValueError('bad types of bookmarks or heads')

            result = backupstate()
            result.heads = set(map(str, state['heads']))
            result.localbookmarks = state['bookmarks']
            return result
        except (ValueError, KeyError, TypeError) as e:
            ui.warn(_('corrupt file: %s (%s)\n') % (_backupstatefile, e))
            return backupstate()
    return backupstate()

def _writelocalbackupstate(vfs, heads, bookmarks):
    with vfs(_backupstatefile, 'w') as f:
        f.write(json.dumps({'heads': list(heads), 'bookmarks': bookmarks}))

def _readbackupgenerationfile(vfs):
    try:
        with vfs(_backupgenerationfile) as f:
            return int(f.read())
    except (IOError, OSError, ValueError):
        return 0

def _writebackupgenerationfile(vfs, backupgenerationvalue):
    with vfs(_backupgenerationfile, 'w', atomictemp=True) as f:
        f.write(str(backupgenerationvalue))

def _writelocalbackupinfo(vfs, rev, time):
    with vfs(_backuplatestinfofile, 'w', atomictemp=True) as f:
        f.write(('backuprevision=%d\nbackuptime=%d\n') % (rev, time))

# Restore helper functions
def _parsebackupbookmark(backupbookmark, namingmgr):
    '''Parses backup bookmark and returns info about it

    Backup bookmark may represent either a local bookmark or a head.
    Returns None if backup bookmark has wrong format or tuple.
    First entry is a hostname where this bookmark came from.
    Second entry is a root of the repo where this bookmark came from.
    Third entry in a tuple is local bookmark if backup bookmark
    represents a local bookmark and None otherwise.
    '''

    backupbookmarkprefix = namingmgr._getcommonuserprefix()
    commonre = '^{0}/([-\w.]+)(/.*)'.format(re.escape(backupbookmarkprefix))
    bookmarkre = commonre + '/bookmarks/(.*)$'
    headsre = commonre + '/heads/[a-f0-9]{40}$'

    match = re.search(bookmarkre, backupbookmark)
    if not match:
        match = re.search(headsre, backupbookmark)
        if not match:
            return None
        # It's a local head not a local bookmark.
        # That's why localbookmark is None
        return backupbookmarktuple(hostname=match.group(1),
                                   reporoot=match.group(2),
                                   localbookmark=None)

    return backupbookmarktuple(hostname=match.group(1),
                               reporoot=match.group(2),
                               localbookmark=_unescapebookmark(match.group(3)))

_timeformat = '%Y%m%d'

def _getlogfilename(logdir, username, reponame):
    '''Returns name of the log file for particular user and repo

    Different users have different directories inside logdir. Log filename
    consists of reponame (basename of repo path) and current day
    (see _timeformat). That means that two different repos with the same name
    can share the same log file. This is not a big problem so we ignore it.
    '''

    currentday = time.strftime(_timeformat)
    return os.path.join(logdir, username, reponame + currentday)

def _removeoldlogfiles(userlogdir, reponame):
    existinglogfiles = []
    for entry in osutil.listdir(userlogdir):
        filename = entry[0]
        fullpath = os.path.join(userlogdir, filename)
        if filename.startswith(reponame) and os.path.isfile(fullpath):
            try:
                time.strptime(filename[len(reponame):], _timeformat)
            except ValueError:
                continue
            existinglogfiles.append(filename)

    # _timeformat gives us a property that if we sort log file names in
    # descending order then newer files are going to be in the beginning
    existinglogfiles = sorted(existinglogfiles, reverse=True)
    # Delete logs that are older than 5 days
    maxlogfilenumber = 5
    if len(existinglogfiles) > maxlogfilenumber:
        for filename in existinglogfiles[maxlogfilenumber:]:
            os.unlink(os.path.join(userlogdir, filename))

def _checkcommonlogdir(logdir):
    '''Checks permissions of the log directory

    We want log directory to actually be a directory, have restricting
    deletion flag set (sticky bit)
    '''

    try:
        st = os.stat(logdir)
        return stat.S_ISDIR(st.st_mode) and st.st_mode & stat.S_ISVTX
    except OSError:
        # is raised by os.stat()
        return False

def _checkuserlogdir(userlogdir):
    '''Checks permissions of the user log directory

    We want user log directory to be writable only by the user who created it
    and be owned by `username`
    '''

    try:
        st = os.stat(userlogdir)
        # Check that `userlogdir` is owned by `username`
        if os.getuid() != st.st_uid:
            return False
        return ((st.st_mode & (stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH)) ==
                stat.S_IWUSR)
    except OSError:
        # is raised by os.stat()
        return False

def _dictdiff(first, second):
    '''Returns new dict that contains items from the first dict that are missing
    from the second dict.
    '''
    result = {}
    for book, hexnode in first.items():
        if second.get(book) != hexnode:
            result[book] = hexnode
    return result

def _getsrcrepo(repo):
    '''returns main repo in case of shared woking copy
    '''
    if repo.sharedpath == repo.path:
        return repo

    # the sharedpath always ends in the .hg; we want the path to the repo
    source = repo.vfs.split(repo.sharedpath)[0]
    srcurl, branches = hg.parseurl(source)
    srcrepo = hg.repository(repo.ui, srcurl)
    if srcrepo.local():
        return srcrepo
    else:
        return repo
