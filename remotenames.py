import os
import errno

from mercurial import commands
from mercurial import encoding
from mercurial import error
from mercurial import exchange
from mercurial import extensions
from mercurial import hg
from mercurial import namespaces
from mercurial import repoview
from mercurial import revset
from mercurial import templatekw
from mercurial import ui
from mercurial import url
from mercurial import util
from mercurial.node import hex
from hgext import schemes
from mercurial import bookmarks

_remotenames = {
    "bookmarks": {},
    "branches": {},
}

def expush(orig, repo, remote, *args, **kwargs):
    # hack for pushing that turns off the dynamic blockerhook
    repo.__setattr__('_hackremotenamepush', True)

    res = orig(repo, remote, *args, **kwargs)
    lock = repo.lock()
    try:
        try:
            path = activepath(repo.ui, remote)
            if path:
                # on a push, we don't want to keep obsolete heads since
                # they won't show up as heads on the next pull, so we
                # remove them here otherwise we would require the user
                # to issue a pull to refresh .hg/remotenames
                bmap = {}
                repo = repo.unfiltered()
                for branch, nodes in remote.branchmap().iteritems():
                    bmap[branch] = [n for n in nodes if not repo[n].obsolete()]
                saveremotenames(repo, path, bmap, remote.listkeys('bookmarks'))
        except Exception, e:
            ui.debug('remote branches for path %s not saved: %s\n'
                     % (path, e))
    finally:
        repo.__setattr__('_hackremotenamepush', False)
        lock.release()
        return res

def expull(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    pullremotenames(repo, remote)
    writedistance(repo)
    return res

def pullremotenames(repo, remote):
    lock = repo.lock()
    try:
        try:
            path = activepath(repo.ui, remote)
            if path:
                saveremotenames(repo, path, remote.branchmap(),
                                remote.listkeys('bookmarks'))
        except Exception, e:
            ui.debug('remote branches for path %s not saved: %s\n'
                     % (path, e))
    finally:
        lock.release()

def blockerhook(orig, repo, *args, **kwargs):
    blockers = orig(repo)

    # protect un-hiding changesets behind a config knob
    nohide = not repo.ui.configbool('remotenames', 'unhide')
    hackpush = util.safehasattr(repo, '_hackremotenamepush')
    if nohide or (hackpush and repo._hackremotenamepush):
        return blockers

    # add remotenames to blockers by looping over all names in our own cache
    cl = repo.changelog
    for remotename in _remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            blockers.update(cl.rev(node) for node in ns.nodes(repo, name))

    return blockers

def exupdatefromremote(orig, ui, repo, remotemarks, path, trfunc, explicit=()):
    if ui.configbool('remotenames', 'syncbookmarks', False):
        return orig(ui, repo, remotemarks, path, trfunc, explicit)

    ui.status('remotenames: skipped syncing local bookmarks\n')

def exclone(orig, ui, *args, **opts):
    """
    We may not want local bookmarks on clone... but we always want remotenames!
    """
    srcpeer, dstpeer = orig(ui, *args, **opts)

    pullremotenames(dstpeer.local(), srcpeer)

    if not ui.configbool('remotenames', 'syncbookmarks', False):
        ui.status('remotenames: removing cloned bookmarks\n')
        repo = dstpeer.local()
        wlock = repo.wlock()
        try:
            try:
                repo.vfs.unlink('bookmarks')
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise
        finally:
            wlock.release()

    return (srcpeer, dstpeer)

extensions.wrapfunction(exchange, 'push', expush)
extensions.wrapfunction(exchange, 'pull', expull)
extensions.wrapfunction(repoview, '_getdynamicblockers', blockerhook)
extensions.wrapfunction(bookmarks, 'updatefromremote', exupdatefromremote)
extensions.wrapfunction(hg, 'clone', exclone)

def reposetup(ui, repo):
    if not repo.local():
        return

    loadremotenames(repo)

    # cache this so we don't iterate over new values
    items = list(repo.names.iteritems())
    for nsname, ns in items:
        d = _remotenames.get(nsname)
        if not d:
            continue

        rname = 'remote' + nsname
        rtmpl = 'remote' + ns.templatename
        names = lambda rp, d=d: d.keys()
        namemap = lambda rp, name, d=d: d.get(name)
        nodemap = lambda rp, node, d=d: [name for name, n in d.iteritems()
                                         for n2 in n if n2 == node]

        n = namespaces.namespace(rname, templatename=rtmpl,
                                 logname=ns.templatename, colorname=rtmpl,
                                 listnames=names, namemap=namemap,
                                 nodemap=nodemap)
        repo.names.addnamespace(n)

def extsetup(ui):
    entry = extensions.wrapcommand(commands.table, 'bookmarks', exbookmarks)
    entry[1].append(('a', 'all', None, 'show both remote and local bookmarks'))
    entry[1].append(('', 'remote', None, 'show only remote bookmarks'))

    entry = extensions.wrapcommand(commands.table, 'branches', exbranches)
    entry[1].append(('a', 'all', None, 'show both remote and local branches'))
    entry[1].append(('', 'remote', None, 'show only remote branches'))

def exbranches(orig, ui, repo, *args, **opts):
    if not opts.get('remote'):
        orig(ui, repo, *args, **opts)

    if opts.get('all') or opts.get('remote'):
        # exit early if namespace doesn't even exist
        namespace = 'remotebranches'
        if namespace not in repo.names:
            return

        ns = repo.names[namespace]
        label = 'log.' + ns.colorname
        fm = ui.formatter('branches', opts)

        # it seems overkill to hide displaying hidden remote branches
        repo = repo.unfiltered()

        # create a sorted by descending rev list
        revs = set()
        for name in ns.listnames(repo):
            for n in ns.nodes(repo, name):
                revs.add(repo.changelog.rev(n))

        for r in sorted(revs, reverse=True):
            ctx = repo[r]
            for name in ns.names(repo, ctx.node()):
                fm.startitem()
                padsize = max(31 - len(str(r)) - encoding.colwidth(name), 0)

                tmplabel = label
                if ctx.obsolete():
                    tmplabel = tmplabel + ' changeset.obsolete'
                fm.write(ns.colorname, '%s', name, label=label)
                fmt = ' ' * padsize + ' %d:%s'
                fm.condwrite(not ui.quiet, 'rev node', fmt, r,
                             fm.hexfunc(ctx.node()), label=tmplabel)
                fm.plain('\n')
        fm.end()

def exbookmarks(orig, ui, repo, *args, **opts):
    """Bookmark output is sorted by bookmark name.

    This has the side benefit of grouping all remote bookmarks by remote name.

    """
    if not opts.get('remote'):
        orig(ui, repo, *args, **opts)

    if opts.get('all') or opts.get('remote'):
        n = 'remotebookmarks'
        if n not in repo.names:
            return
        ns = repo.names[n]
        color = ns.colorname
        label = 'log.' + color

        fm = ui.formatter('bookmarks', opts)

        # it seems overkill to hide displaying hidden remote bookmarks
        repo = repo.unfiltered()

        for name in sorted(ns.listnames(repo)):
            node = ns.nodes(repo, name)[0]
            ctx = repo[node]
            fm.startitem()

            if not ui.quiet:
                fm.plain('   ')

            padsize = max(25 - encoding.colwidth(name), 0)
            fmt = ' ' * padsize + ' %d:%s'

            tmplabel = label
            if ctx.obsolete():
                tmplabel = tmplabel + ' changeset.obsolete'
            fm.write(color, '%s', name, label=label)
            fm.condwrite(not ui.quiet, 'rev node', fmt, ctx.rev(),
                         fm.hexfunc(node), label=tmplabel)
            fm.plain('\n')

def activepath(ui, remote):
    realpath = ''
    local = None
    try:
        local = remote.local()
    except AttributeError:
        pass

    # determine the remote path from the repo, if possible; else just
    # use the string given to us
    rpath = remote
    if local:
        rpath = getattr(remote, 'root', None)
        if rpath is None:
            # Maybe a localpeer? (hg@1ac628cd7113, 2.3)
            rpath = getattr(getattr(remote, '_repo', None),
                            'root', None)
    elif not isinstance(remote, str):
        try:
            rpath = remote._url
        except:
            rpath = remote.url

    for path, uri in ui.configitems('paths'):
        uri = ui.expandpath(expandscheme(ui, uri))
        if local:
            uri = os.path.realpath(uri)
        else:
            if uri.startswith('http'):
                try:
                    uri = url.url(uri).authinfo()[0]
                except AttributeError:
                    try:
                        uri = util.url(uri).authinfo()[0]
                    except AttributeError:
                        uri = url.getauthinfo(uri)[0]
        uri = uri.rstrip('/')
        rpath = rpath.rstrip('/')
        if uri == rpath:
            realpath = path
            # prefer a non-default name to default
            if path != 'default' and path != 'default-push':
                break
    return realpath

def expandscheme(ui, uri):
    '''For a given uri, expand the scheme for it'''
    urischemes = [s for s in schemes.schemes.iterkeys()
                  if uri.startswith('%s://' % s)]
    for s in urischemes:
        # TODO: refactor schemes so we don't
        # duplicate this logic
        ui.note('performing schemes expansion with '
                'scheme %s\n' % s)
        scheme = hg.schemes[s]
        parts = uri.split('://', 1)[1].split('/', scheme.parts)
        if len(parts) > scheme.parts:
            tail = parts[-1]
            parts = parts[:-1]
        else:
            tail = ''
        ctx = dict((str(i + 1), v) for i, v in enumerate(parts))
        uri = ''.join(scheme.templater.process(scheme.url, ctx)) + tail
    return uri

def splitremotename(remote):
    name = ''
    if '/' in remote:
        remote, name = remote.split('/', 1)
    return remote, name

def joinremotename(remote, ref):
    if ref:
        remote += '/' + ref
    return remote

def readremotenames(repo):
    rfile = repo.join('remotenames')
    # exit early if there is nothing to do
    if not os.path.exists(rfile):
        return

    # needed to heuristically determine if a file is in the old format
    branches = repo.names['branches'].listnames(repo)
    bookmarks = repo.names['bookmarks'].listnames(repo)

    f = open(rfile)
    for line in f:
        nametype = None
        line = line.strip()
        if not line:
            continue
        nametype = None
        remote, rname = None, None

        node, name = line.split(' ', 1)

        # check for nametype being written into the file format
        if ' ' in name:
            nametype, name = name.split(' ', 1)

        remote, rname = splitremotename(name)

        # skip old data that didn't write the name (only wrote the alias)
        if not rname:
            continue

        # old format didn't save the nametype, so check for the name in
        # branches and bookmarks
        if nametype is None:
            if rname in branches:
                nametype = 'branches'
            elif rname in bookmarks:
                nametype = 'bookmarks'

        yield node, nametype, remote, rname

    f.close()

def loadremotenames(repo):
    alias_default = repo.ui.configbool('remotenames', 'alias.default')

    for node, nametype, remote, rname in readremotenames(repo):
        # handle alias_default here
        if remote != "default" and rname == "default" and alias_default:
            name = remote
        else:
            name = joinremotename(remote, rname)

        # if the node doesn't exist, skip it
        try:
            ctx = repo[node]
        except error.RepoLookupError:
            continue

        # only mark as remote if the head changeset isn't marked closed
        if not ctx.extra().get('close'):
            nodes = _remotenames[nametype].get(name, [])
            nodes.append(ctx.node())
            _remotenames[nametype][name] = nodes

def saveremotenames(repo, remote, branches, bookmarks):
    # read in all data first before opening file to write
    olddata = set(readremotenames(repo))

    bfile = repo.join('remotenames')
    f = open(bfile, 'w')

    # only update the given 'remote', so iterate over old data and re-save it
    for node, nametype, oldremote, rname in olddata:
        if oldremote != remote:
            n = joinremotename(oldremote, rname)
            f.write('%s %s %s\n' % (node, nametype, n))

    for branch, nodes in branches.iteritems():
        for n in nodes:
            rname = joinremotename(remote, branch)
            f.write('%s branches %s\n' % (hex(n), rname))
    for bookmark, n in bookmarks.iteritems():
        f.write('%s bookmarks %s\n' % (n, joinremotename(remote, bookmark)))
    f.close()

def distancefromremote(repo, remote="default"):
    """returns the signed distance between the current node and remote"""
    b = repo._bookmarkcurrent

    # if no bookmark is active, fallback to the branchname
    if not b:
        b = repo.lookupbranch('.')

    # get the non-default name
    paths = dict(repo.ui.configitems('paths'))
    rpath = paths.get(remote)
    if remote == 'default':
        for path, uri in paths.iteritems():
            if path != 'default' and path != 'default-push' and rpath == uri:
                remote = path

    # if we couldn't find anything for remote then return
    if not rpath:
        return 0

    remoteb = joinremotename(remote, b)
    distance = 0

    if remoteb in repo:
        rev1 = repo[remoteb].rev()
        rev2 = repo['.'].rev()
        sign = 1
        if rev2 < rev1:
            sign = -1
            rev1, rev2 = rev2, rev1
        nodes = repo.revs('%s::%s' % (rev1, rev2))
        distance = sign * (len(nodes) - 1)

    return distance

def writedistance(repo, remote="default"):
    distance = distancefromremote(repo, remote)
    sign = '+'
    if distance < 0:
        sign = '-'

    wlock = repo.wlock()
    try:
        try:
            fp = repo.vfs('remotedistance', 'w', atomictemp=True)
            fp.write('%s %s' % (sign, abs(distance)))
            fp.close()
        except OSError, inst:
            if inst.errno != errno.ENOENT:
                raise
    finally:
        wlock.release()

#########
# revsets
#########

def upstream_revs(filt, repo, subset, x):
    upstream_tips = set()
    for remotename in _remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            if filt(name):
                upstream_tips.update(ns.nodes(repo, name))

    if not upstream_tips:
        return revset.baseset([])

    tipancestors = repo.revs('::%ln', upstream_tips)
    return revset.filteredset(subset, lambda n: n in tipancestors)

def upstream(repo, subset, x):
    '''``upstream()``
    Select changesets in an upstream repository according to remotenames.
    '''
    revset.getargs(x, 0, 0, "upstream takes no arguments")
    upstream_names = [s + '/' for s in
                      repo.ui.configlist('remotenames', 'upstream')]
    if not upstream_names:
        filt = lambda x: True
    else:
        filt = lambda name: any(map(name.startswith, upstream_names))
    return upstream_revs(filt, repo, subset, x)

def pushed(repo, subset, x):
    '''``pushed()``
    Select changesets in any remote repository according to remotenames.
    '''
    revset.getargs(x, 0, 0, "pushed takes no arguments")
    return upstream_revs(lambda x: True, repo, subset, x)

def remotenamesrevset(repo, subset, x):
    """``remotenames()``
    All remote branches heads.
    """
    revset.getargs(x, 0, 0, "remotenames takes no arguments")
    remoterevs = set()
    cl = repo.changelog
    for remotename in _remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            remoterevs.update(ns.nodes(repo, name))

    return revset.baseset(sorted(cl.rev(n) for n in remoterevs))

revset.symbols.update({'upstream': upstream,
                       'pushed': pushed,
                       'remotenames': remotenamesrevset})

###########
# templates
###########

def remotenameskw(**args):
    """:remotenames: List of strings. List of remote names associated with the
    changeset. If remotenames.suppressbranches is True then branch names will
    be hidden if there is a bookmark at the same changeset.

    """
    repo, ctx = args['repo'], args['ctx']

    remotenames = []
    if 'remotebookmarks' in repo.names:
        remotenames = repo.names['remotebookmarks'].names(repo, ctx.node())

    suppress = repo.ui.configbool('remotenames', 'suppressbranches', False)
    if (not remotenames or not suppress) and 'remotebranches' in repo.names:
        remotenames += repo.names['remotebranches'].names(repo, ctx.node())

    return templatekw.showlist('remotename', remotenames,
                               plural='remotenames', **args)

templatekw.keywords['remotenames'] = remotenameskw
