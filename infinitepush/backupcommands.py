# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import
import hashlib
import os
import re
import socket

from .bundleparts import (
    getscratchbookmarkspart,
    getscratchbranchpart,
)
from mercurial import (
    bundle2,
    cmdutil,
    commands,
    discovery,
    encoding,
    error,
    hg,
    util,
)

from collections import namedtuple
from hgext3rd.extutil import runshellcommand
from mercurial.node import bin, hex
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

backupbookmarktuple = namedtuple('backupbookmarktuple',
                                 ['hostname', 'reporoot', 'localbookmark'])

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
        logfile = ui.config('infinitepush', 'pushbackuplog')
        if logfile:
            background_cmd.extend(('>>', logfile, '2>&1'))
        runshellcommand(' '.join(background_cmd), os.environ)
        return 0

    backuptip, bookmarkshash = _readbackupstatefile(ui, repo)
    bookmarkstobackup = _getbookmarkstobackup(ui, repo)

    # To avoid race conditions save current tip of the repo and backup
    # everything up to this revision.
    currenttiprev = len(repo) - 1
    other = _getremote(repo, ui, dest, **opts)
    outgoing = _getrevstobackup(repo, other, backuptip,
                                currenttiprev, bookmarkstobackup)
    currentbookmarkshash = _getbookmarkshash(bookmarkstobackup)

    bundler = _createbundler(ui, repo, other)
    backup = False
    if outgoing and outgoing.missing:
        backup = True
        bundler.addpart(getscratchbranchpart(repo, other, outgoing,
                                             confignonforwardmove=False,
                                             ui=ui, bookmark=None,
                                             create=False))

    if currentbookmarkshash != bookmarkshash:
        backup = True
        bundler.addpart(getscratchbookmarkspart(other, bookmarkstobackup))

    if backup:
        _sendbundle(bundler, other)
        _writebackupstatefile(repo.svfs, currenttiprev, currentbookmarkshash)
    else:
        ui.status(_('nothing to backup\n'))
    return 0

@command('pullbackup', [
         ('', 'reporoot', '', 'root of the repo to restore'),
         ('', 'hostname', '', 'hostname of the repo to restore')])
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

    pattern = _getcommonuserprefix(ui) + '/*'
    fetchedbookmarks = other.listkeyspatterns('bookmarks', patterns=[pattern])
    reporoots = set()
    hostnames = set()
    nodestopull = set()
    localbookmarks = {}
    for book, node in fetchedbookmarks.iteritems():
        parsed = _parsebackupbookmark(ui, book)
        if parsed:
            if sourcereporoot and sourcereporoot != parsed.reporoot:
                continue
            if sourcehostname and sourcehostname != parsed.hostname:
                continue
            nodestopull.add(node)
            if parsed.localbookmark:
                localbookmarks[parsed.localbookmark] = node
            reporoots.add(parsed.reporoot)
            hostnames.add(parsed.hostname)
        else:
            ui.warn(_('wrong format of backup bookmark: %s') % book)

    if len(reporoots) > 1:
        raise error.Abort(
            _('ambiguous repo root to restore: %s') % sorted(reporoots),
            hint=_('set --reporoot to disambiguate'))

    if len(hostnames) > 1:
        raise error.Abort(
            _('ambiguous hostname to restore: %s') % sorted(hostnames),
            hint=_('set --hostname to disambiguate'))
    pullcmd, pullopts = _getcommandandoptions('^pull')
    pullopts['rev'] = list(nodestopull)
    result = pullcmd(ui, repo, **pullopts)

    with repo.wlock():
        with repo.lock():
            with repo.transaction('bookmark') as tr:
                for scratchbook, hexnode in localbookmarks.iteritems():
                    repo._bookmarks[scratchbook] = bin(hexnode)
                repo._bookmarks.recordchange(tr)

    return result

_backupedstatefile = 'infinitepushlastbackupedstate'

# Common helper functions

def _getcommonuserprefix(ui):
    username = ui.shortuser(ui.username())
    return '/'.join(('infinitepush', 'backups', username))

def _getcommonprefix(ui, repo):
    hostname = socket.gethostname()

    result = '/'.join((_getcommonuserprefix(ui), hostname))
    if not repo.origroot.startswith('/'):
        result += '/'
    result += repo.origroot
    if result.endswith('/'):
        result = result[:-1]
    return result

def _getbackupbookmarkprefix(ui, repo):
    return '/'.join((_getcommonprefix(ui, repo),
                     'bookmarks'))

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

def _getbackupbookmarkname(ui, bookmark, repo):
    bookmark = _escapebookmark(bookmark)
    return '/'.join((_getbackupbookmarkprefix(ui, repo), bookmark))

def _getbackupheadprefix(ui, repo):
    return '/'.join((_getcommonprefix(ui, repo),
                     'heads'))

def _getbackupheadname(ui, hexhead, repo):
    return '/'.join((_getbackupheadprefix(ui, repo), hexhead))

def _getremote(repo, ui, dest, **opts):
    path = ui.paths.getpath(dest, default=('default-push', 'default'))
    if not path:
        raise error.Abort(_('default repository not configured!'),
                         hint=_("see 'hg help config.paths'"))
    dest = path.pushloc or path.loc
    return hg.peer(repo, opts, dest)

def _getcommandandoptions(command):
    pushcmd = commands.table[command][0]
    pushopts = dict(opt[1:3] for opt in commands.table[command][1])
    return pushcmd, pushopts

# Backup helper functions

def _getdefaultbookmarkstobackup(ui, repo):
    bookmarkstobackup = {}
    bookmarkstobackup[_getbackupheadprefix(ui, repo) + '/*'] = ''
    bookmarkstobackup[_getbackupbookmarkprefix(ui, repo) + '/*'] = ''
    return bookmarkstobackup

def _getbookmarkstobackup(ui, repo):
    bookmarkstobackup = _getdefaultbookmarkstobackup(ui, repo)
    for bookmark, node in repo._bookmarks.iteritems():
        bookmark = _getbackupbookmarkname(ui, bookmark, repo)
        hexnode = hex(node)
        bookmarkstobackup[bookmark] = hexnode

    for headrev in repo.revs('head() & not public()'):
        hexhead = repo[headrev].hex()
        headbookmarksname = _getbackupheadname(ui, hexhead, repo)
        bookmarkstobackup[headbookmarksname] = hexhead

    return bookmarkstobackup

def _getbookmarkshash(bookmarkstobackup):
    currentbookmarkshash = hashlib.sha1()
    for book, node in sorted(bookmarkstobackup.iteritems()):
        currentbookmarkshash.update(book)
        currentbookmarkshash.update(node)
    return currentbookmarkshash.hexdigest()

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

def _getrevstobackup(repo, other, backuptip, currenttiprev, bookmarkstobackup):
    # Use unfiltered repo because backuptip may now point to filtered commit
    repo = repo.unfiltered()
    revs = []
    if backuptip <= currenttiprev:
        revset = 'head() & draft() & %d:' % backuptip
        revs = list(repo.revs(revset))

    outgoing = findcommonoutgoing(repo, other, revs)

    return outgoing

def _readbackupstatefile(ui, repo):
    backuptipbookmarkshash = repo.svfs.tryread(_backupedstatefile).split(' ')
    backuptip = 0
    # hash of the default bookmarks to backup. This is to prevent backuping of
    # empty repo
    bookmarkshash = _getbookmarkshash(_getdefaultbookmarkstobackup(ui, repo))
    if len(backuptipbookmarkshash) == 2:
        try:
            backuptip = int(backuptipbookmarkshash[0]) + 1
        except ValueError:
            pass
        if len(backuptipbookmarkshash[1]) == 40:
            bookmarkshash = backuptipbookmarkshash[1]
    return backuptip, bookmarkshash

def _writebackupstatefile(vfs, backuptip, bookmarkshash):
    with vfs(_backupedstatefile, mode="w", atomictemp=True) as f:
        f.write(str(backuptip) + ' ' + bookmarkshash)

# Restore helper functions
def _parsebackupbookmark(ui, backupbookmark):
    '''Parses backup bookmark and returns info about it

    Backup bookmark may represent either a local bookmark or a head.
    Returns None if backup bookmark has wrong format or tuple.
    First entry is a hostname where this bookmark came from.
    Second entry is a root of the repo where this bookmark came from.
    Third entry in a tuple is local bookmark if backup bookmark
    represents a local bookmark and None otherwise.
    '''

    commonre = '^{0}/([-\w.]+)(/.*)'.format(re.escape(_getcommonuserprefix(ui)))
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
