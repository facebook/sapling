# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
    [infinitepushbackup]
    # path to the directory where pushback logs should be stored
    logdir = path/to/dir

    # max number of logs for one repo for one user
    maxlognumber = 5

    # There can be at most one backup process per repo. This config options
    # determines how much time to wait on the lock. If timeout happens then
    # backups process aborts.
    waittimeout = 30

    # Backup at most maxheadstobackup heads, other heads are ignored.
    # Negative number means backup everything.
    maxheadstobackup = -1
"""

from __future__ import absolute_import
import errno
import json
import os
import re
import socket
import time

from .bundleparts import (
    getscratchbookmarkspart,
    getscratchbranchpart,
)
from mercurial import (
    bundle2,
    changegroup,
    cmdutil,
    commands,
    discovery,
    encoding,
    error,
    hg,
    lock as lockmod,
    osutil,
    phases,
    util,
)

from collections import defaultdict, namedtuple
from hgext3rd.extutil import runshellcommand
from mercurial.extensions import wrapfunction, unwrapfunction
from mercurial.node import bin, hex, nullrev
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

backupbookmarktuple = namedtuple('backupbookmarktuple',
                                 ['hostname', 'reporoot', 'localbookmark'])

class backupstate(object):
    def __init__(self):
        self.heads = set()
        self.localbookmarks = {}

    def empty(self):
        return not self.heads and not self.localbookmarks

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
        logdir = ui.config('infinitepushbackup', 'logdir')
        if logdir:
            try:
                try:
                    username = util.shortuser(ui.username())
                except Exception:
                    username = 'unknown'
                userlogdir = os.path.join(logdir, username)
                util.makedirs(userlogdir)
                reporoot = repo.origroot
                reponame = os.path.basename(reporoot)

                maxlogfilenumber = ui.configint('infinitepushbackup',
                                                'maxlognumber', 5)
                _removeoldlogfiles(userlogdir, reponame, maxlogfilenumber)
                logfile = _getlogfilename(logdir, username, reponame)
                background_cmd.extend(('>>', logfile, '2>&1'))
            except (OSError, IOError) as e:
                ui.warn(_('infinitepush backup log is disabled: %s\n') % e)
        runshellcommand(' '.join(background_cmd), os.environ)
        return 0

    try:
        timeout = ui.configint('infinitepushbackup', 'waittimeout', 30)
        with lockmod.lock(repo.vfs, _backuplockname, timeout=timeout):
            return _dobackup(ui, repo, dest, **opts)
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            ui.warn(_('timeout waiting on backup lock'))
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
    username = opts.get('user') or util.shortuser(ui.username())

    allbackupstates = _downloadbackupstate(ui, other, sourcereporoot,
                                           sourcehostname, username)
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

    with repo.wlock():
        with repo.lock():
            with repo.transaction('bookmark') as tr:
                for book, hexnode in backupstate.localbookmarks.iteritems():
                    if hexnode in repo:
                        repo._bookmarks[book] = bin(hexnode)
                    else:
                        ui.warn(_('%s not found, not creating %s bookmark') %
                                (hexnode, book))
                repo._bookmarks.recordchange(tr)

    return result

@command('getavailablebackups',
    [('', 'user', '', _('username, defaults to current user')),
     ('', 'json', None, _('print available backups in json format'))])
def getavailablebackups(ui, repo, dest=None, **opts):
    other = _getremote(repo, ui, dest, **opts)

    sourcereporoot = opts.get('reporoot')
    sourcehostname = opts.get('hostname')
    username = opts.get('user') or ui.shortuser(ui.username())

    allbackupstates = _downloadbackupstate(ui, other, sourcereporoot,
                                           sourcehostname, username)

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
            ui.write(_('no backups available for %s\n') % username)

        ui.write(_('user %s has %d available backups:\n') %
                 (username, len(allbackupstates)))

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
    username = opts.get('user') or util.shortuser(ui.username())

    other = _getremote(repo, ui, dest, **opts)
    allbackupstates = _downloadbackupstate(ui, other, sourcereporoot,
                                           sourcehostname, username)
    if not opts.get('all'):
        _checkbackupstates(allbackupstates)

    ret = 0
    while allbackupstates:
        # recreate remote because `lookup` request might have failed and
        # connection was closed
        other = _getremote(repo, ui, dest, **opts)
        key, bkpstate = allbackupstates.popitem()
        ui.status(_('checking %s on %s\n') % (key[1], key[0]))
        batch = other.iterbatch()
        for hexnode in list(bkpstate.heads) + bkpstate.localbookmarks.values():
            batch.lookup(hexnode)
        batch.submit()
        lookupresults = batch.results()
        try:
            for r in lookupresults:
                # iterate over results to make it throw if revision
                # was not found
                pass
        except error.RepoError as e:
            ui.warn(_('%s\n') % e)
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
        with lockmod.lock(repo.vfs, _backuplockname, timeout=timeout):
            pass
    except error.LockHeld as e:
        if e.errno == errno.ETIMEDOUT:
            raise error.Abort(_('timeout while waiting for backup'))
        raise

def _dobackup(ui, repo, dest, **opts):
    ui.status(_('starting backup %s\n') % time.strftime('%H:%M:%S %d %b %Y %Z'))
    start = time.time()
    username = util.shortuser(ui.username())
    bkpstate = _readlocalbackupstate(ui, repo)

    maxheadstobackup = ui.configint('infinitepushbackup',
                                    'maxheadstobackup', -1)

    revset = 'head() & draft() & not extinct()'
    # This variable will store what heads will be saved in backup state file
    # if backup finishes successfully
    afterbackupheads = [ctx.hex() for ctx in repo.set(revset)]
    if maxheadstobackup > 0:
        afterbackupheads = afterbackupheads[-maxheadstobackup:]
    elif maxheadstobackup == 0:
        afterbackupheads = []
    afterbackupheads = set(afterbackupheads)
    other = _getremote(repo, ui, dest, **opts)
    outgoing, badhexnodes = _getrevstobackup(repo, other,
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

    bookmarkstobackup = _getbookmarkstobackup(
        username, repo, newbookmarks, removedbookmarks,
        newheads, removedheads)

    # Special case if backup state is empty. Clean all backup bookmarks from the
    # server.
    if bkpstate.empty():
        bookmarkstobackup[_getbackupheadprefix(username, repo) + '/*'] = ''
        bookmarkstobackup[_getbackupbookmarkprefix(username, repo) + '/*'] = ''

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

_backupstatefile = 'infinitepushbackupstate'

# Common helper functions

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

def _downloadbackupstate(ui, other, sourcereporoot, sourcehostname, username):
    pattern = _getcommonuserprefix(username) + '/*'
    fetchedbookmarks = other.listkeyspatterns('bookmarks', patterns=[pattern])
    allbackupstates = defaultdict(backupstate)
    for book, hexnode in fetchedbookmarks.iteritems():
        parsed = _parsebackupbookmark(username, book)
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

def _getcommonuserprefix(username):
    return '/'.join(('infinitepush', 'backups', username))

def _getcommonprefix(username, repo):
    hostname = socket.gethostname()

    result = '/'.join((_getcommonuserprefix(username), hostname))
    if not repo.origroot.startswith('/'):
        result += '/'
    result += repo.origroot
    if result.endswith('/'):
        result = result[:-1]
    return result

def _getbackupbookmarkprefix(username, repo):
    return '/'.join((_getcommonprefix(username, repo), 'bookmarks'))

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

def _getbackupbookmarkname(username, bookmark, repo):
    bookmark = _escapebookmark(bookmark)
    return '/'.join((_getbackupbookmarkprefix(username, repo), bookmark))

def _getbackupheadprefix(username, repo):
    return '/'.join((_getcommonprefix(username, repo), 'heads'))

def _getbackupheadname(username, hexhead, repo):
    return '/'.join((_getbackupheadprefix(username, repo), hexhead))

def _getremote(repo, ui, dest, **opts):
    path = ui.paths.getpath(dest, default=('default-push', 'default'))
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

def _getbookmarkstobackup(username, repo, newbookmarks, removedbookmarks,
                          newheads, removedheads):
    bookmarkstobackup = {}

    for bookmark, hexnode in removedbookmarks.items():
        backupbookmark = _getbackupbookmarkname(username, bookmark, repo)
        bookmarkstobackup[backupbookmark] = ''

    for bookmark, hexnode in newbookmarks.items():
        backupbookmark = _getbackupbookmarkname(username, bookmark, repo)
        bookmarkstobackup[backupbookmark] = hexnode

    for hexhead in removedheads:
        headbookmarksname = _getbackupheadname(username, hexhead, repo)
        bookmarkstobackup[headbookmarksname] = ''

    for hexhead in newheads:
        headbookmarksname = _getbackupheadname(username, hexhead, repo)
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

def _getrevstobackup(repo, other, headstobackup):
    revs = list(repo[hexnode].rev() for hexnode in headstobackup)

    outgoing = findcommonoutgoing(repo, other, revs)
    rootstofilter = []
    if outgoing:
        # In rare cases it's possible to have node without filelogs only
        # locally. It is possible if remotefilelog is enabled and if node was
        # stripped server-side. In this case we want to filter this
        # nodes and all ancestors out
        for node in outgoing.missing:
            changectx = repo[node]
            for file in changectx.files():
                try:
                    changectx.filectx(file)
                except error.ManifestLookupError:
                    rootstofilter.append(changectx.rev())

    badhexnodes = set()
    if rootstofilter:
        revstofilter = list(repo.revs('%ld::', rootstofilter))
        badhexnodes = set(repo[rev].hex() for rev in revstofilter)
        revs = set(revs) - set(revstofilter)
        outgoing = findcommonoutgoing(repo, other, revs)

    return outgoing, badhexnodes

def _readlocalbackupstate(ui, repo):
    if not repo.vfs.exists(_backupstatefile):
        return backupstate()

    errormsg = 'corrupt %s file' % _backupstatefile
    with repo.vfs(_backupstatefile) as f:
        try:
            state = json.loads(f.read())
            if 'bookmarks' not in state or 'heads' not in state:
                ui.warn(_('%s\n') % errormsg)
                return backupstate()
            if (type(state['bookmarks']) != type({}) or
                    type(state['heads']) != type([])):
                ui.warn(_('%s\n') % errormsg)
                return backupstate()

            result = backupstate()
            result.heads = set(state['heads'])
            result.localbookmarks = state['bookmarks']
            return result
        except ValueError:
            ui.warn(_('%s\n') % errormsg)
            return backupstate()
    return backupstate()

def _writelocalbackupstate(vfs, heads, bookmarks):
    with vfs(_backupstatefile, 'w') as f:
        f.write(json.dumps({'heads': list(heads), 'bookmarks': bookmarks}))

# Restore helper functions
def _parsebackupbookmark(username, backupbookmark):
    '''Parses backup bookmark and returns info about it

    Backup bookmark may represent either a local bookmark or a head.
    Returns None if backup bookmark has wrong format or tuple.
    First entry is a hostname where this bookmark came from.
    Second entry is a root of the repo where this bookmark came from.
    Third entry in a tuple is local bookmark if backup bookmark
    represents a local bookmark and None otherwise.
    '''

    backupbookmarkprefix = _getcommonuserprefix(username)
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

def _removeoldlogfiles(userlogdir, reponame, maxlogfilenumber):
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
    if len(existinglogfiles) > maxlogfilenumber:
        for filename in existinglogfiles[maxlogfilenumber:]:
            os.unlink(os.path.join(userlogdir, filename))

def _dictdiff(first, second):
    '''Returns new dict that contains items from the first dict that are missing
    from the second dict.
    '''
    result = {}
    for book, hexnode in first.items():
        if second.get(book) != hexnode:
            result[book] = hexnode
    return result
