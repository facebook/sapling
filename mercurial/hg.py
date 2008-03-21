# hg.py - repository classes for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
import localrepo, bundlerepo, httprepo, sshrepo, statichttprepo
import errno, lock, os, shutil, util, extensions
import merge as _merge
import verify as _verify

def _local(path):
    return (os.path.isfile(util.drop_scheme('file', path)) and
            bundlerepo or localrepo)

def parseurl(url, revs):
    '''parse url#branch, returning url, branch + revs'''

    if '#' not in url:
        return url, (revs or None), None

    url, rev = url.split('#', 1)
    return url, revs + [rev], rev

schemes = {
    'bundle': bundlerepo,
    'file': _local,
    'http': httprepo,
    'https': httprepo,
    'ssh': sshrepo,
    'static-http': statichttprepo,
}

def _lookup(path):
    scheme = 'file'
    if path:
        c = path.find(':')
        if c > 0:
            scheme = path[:c]
    thing = schemes.get(scheme) or schemes['file']
    try:
        return thing(path)
    except TypeError:
        return thing

def islocal(repo):
    '''return true if repo or path is local'''
    if isinstance(repo, str):
        try:
            return _lookup(repo).islocal(repo)
        except AttributeError:
            return False
    return repo.local()

def repository(ui, path='', create=False):
    """return a repository object for the specified path"""
    repo = _lookup(path).instance(ui, path, create)
    ui = getattr(repo, "ui", ui)
    for name, module in extensions.extensions():
        hook = getattr(module, 'reposetup', None)
        if hook:
            hook(ui, repo)
    return repo

def defaultdest(source):
    '''return default destination of clone if none is given'''
    return os.path.basename(os.path.normpath(source))

def clone(ui, source, dest=None, pull=False, rev=None, update=True,
          stream=False):
    """Make a copy of an existing repository.

    Create a copy of an existing repository in a new directory.  The
    source and destination are URLs, as passed to the repository
    function.  Returns a pair of repository objects, the source and
    newly created destination.

    The location of the source is added to the new repository's
    .hg/hgrc file, as the default to be used for future pulls and
    pushes.

    If an exception is raised, the partly cloned/updated destination
    repository will be deleted.

    Arguments:

    source: repository object or URL

    dest: URL of destination repository to create (defaults to base
    name of source repository)

    pull: always pull from source repository, even in local case

    stream: stream raw data uncompressed from repository (fast over
    LAN, slow over WAN)

    rev: revision to clone up to (implies pull=True)

    update: update working directory after clone completes, if
    destination is local repository
    """

    if isinstance(source, str):
        origsource = ui.expandpath(source)
        source, rev, checkout = parseurl(origsource, rev)
        src_repo = repository(ui, source)
    else:
        src_repo = source
        origsource = source = src_repo.url()
        checkout = None

    if dest is None:
        dest = defaultdest(source)
        ui.status(_("destination directory: %s\n") % dest)

    def localpath(path):
        if path.startswith('file://localhost/'):
            return path[16:]
        if path.startswith('file://'):
            return path[7:]
        if path.startswith('file:'):
            return path[5:]
        return path

    dest = localpath(dest)
    source = localpath(source)

    if os.path.exists(dest):
        raise util.Abort(_("destination '%s' already exists") % dest)

    class DirCleanup(object):
        def __init__(self, dir_):
            self.rmtree = shutil.rmtree
            self.dir_ = dir_
        def close(self):
            self.dir_ = None
        def __del__(self):
            if self.dir_:
                self.rmtree(self.dir_, True)

    src_lock = dest_lock = dir_cleanup = None
    try:
        if islocal(dest):
            dir_cleanup = DirCleanup(dest)

        abspath = origsource
        copy = False
        if src_repo.cancopy() and islocal(dest):
            abspath = os.path.abspath(util.drop_scheme('file', origsource))
            copy = not pull and not rev

        if copy:
            try:
                # we use a lock here because if we race with commit, we
                # can end up with extra data in the cloned revlogs that's
                # not pointed to by changesets, thus causing verify to
                # fail
                src_lock = src_repo.lock()
            except lock.LockException:
                copy = False

        if copy:
            def force_copy(src, dst):
                if not os.path.exists(src):
                    # Tolerate empty source repository and optional files
                    return
                util.copyfiles(src, dst)

            src_store = os.path.realpath(src_repo.spath)
            if not os.path.exists(dest):
                os.mkdir(dest)
            try:
                dest_path = os.path.realpath(os.path.join(dest, ".hg"))
                os.mkdir(dest_path)
            except OSError, inst:
                if inst.errno == errno.EEXIST:
                    dir_cleanup.close()
                    raise util.Abort(_("destination '%s' already exists")
                                     % dest)
                raise
            if src_repo.spath != src_repo.path:
                # XXX racy
                dummy_changelog = os.path.join(dest_path, "00changelog.i")
                # copy the dummy changelog
                force_copy(src_repo.join("00changelog.i"), dummy_changelog)
                dest_store = os.path.join(dest_path, "store")
                os.mkdir(dest_store)
            else:
                dest_store = dest_path
            # copy the requires file
            force_copy(src_repo.join("requires"),
                       os.path.join(dest_path, "requires"))
            # we lock here to avoid premature writing to the target
            dest_lock = lock.lock(os.path.join(dest_store, "lock"))

            files = ("data",
                     "00manifest.d", "00manifest.i",
                     "00changelog.d", "00changelog.i")
            for f in files:
                src = os.path.join(src_store, f)
                dst = os.path.join(dest_store, f)
                force_copy(src, dst)

            # we need to re-init the repo after manually copying the data
            # into it
            dest_repo = repository(ui, dest)

        else:
            try:
                dest_repo = repository(ui, dest, create=True)
            except OSError, inst:
                if inst.errno == errno.EEXIST:
                    dir_cleanup.close()
                    raise util.Abort(_("destination '%s' already exists")
                                     % dest)
                raise

            revs = None
            if rev:
                if 'lookup' not in src_repo.capabilities:
                    raise util.Abort(_("src repository does not support revision "
                                       "lookup and so doesn't support clone by "
                                       "revision"))
                revs = [src_repo.lookup(r) for r in rev]

            if dest_repo.local():
                dest_repo.clone(src_repo, heads=revs, stream=stream)
            elif src_repo.local():
                src_repo.push(dest_repo, revs=revs)
            else:
                raise util.Abort(_("clone from remote to remote not supported"))

        if dir_cleanup:
            dir_cleanup.close()

        if dest_repo.local():
            fp = dest_repo.opener("hgrc", "w", text=True)
            fp.write("[paths]\n")
            fp.write("default = %s\n" % abspath)
            fp.close()

            if update:
                dest_repo.ui.status(_("updating working directory\n"))
                if not checkout:
                    try:
                        checkout = dest_repo.lookup("default")
                    except:
                        checkout = dest_repo.changelog.tip()
                _update(dest_repo, checkout)

        return src_repo, dest_repo
    finally:
        del src_lock, dest_lock, dir_cleanup

def _showstats(repo, stats):
    stats = ((stats[0], _("updated")),
             (stats[1], _("merged")),
             (stats[2], _("removed")),
             (stats[3], _("unresolved")))
    note = ", ".join([_("%d files %s") % s for s in stats])
    repo.ui.status("%s\n" % note)

def _update(repo, node): return update(repo, node)

def update(repo, node):
    """update the working directory to node, merging linear changes"""
    pl = repo.parents()
    stats = _merge.update(repo, node, False, False, None)
    _showstats(repo, stats)
    if stats[3]:
        repo.ui.status(_("There are unresolved merges with"
                         " locally modified files.\n"))
        if stats[1]:
            repo.ui.status(_("You can finish the partial merge using:\n"))
        else:
            repo.ui.status(_("You can redo the full merge using:\n"))
        # len(pl)==1, otherwise _merge.update() would have raised util.Abort:
        repo.ui.status(_("  hg update %s\n  hg update %s\n")
                       % (pl[0].rev(), repo.changectx(node).rev()))
    return stats[3] > 0

def clean(repo, node, show_stats=True):
    """forcibly switch the working directory to node, clobbering changes"""
    stats = _merge.update(repo, node, False, True, None)
    if show_stats: _showstats(repo, stats)
    return stats[3] > 0

def merge(repo, node, force=None, remind=True):
    """branch merge with node, resolving changes"""
    stats = _merge.update(repo, node, True, force, False)
    _showstats(repo, stats)
    if stats[3]:
        pl = repo.parents()
        repo.ui.status(_("There are unresolved merges,"
                         " you can redo the full merge using:\n"
                         "  hg update -C %s\n"
                         "  hg merge %s\n")
                       % (pl[0].rev(), pl[1].rev()))
    elif remind:
        repo.ui.status(_("(branch merge, don't forget to commit)\n"))
    return stats[3] > 0

def revert(repo, node, choose):
    """revert changes to revision in node without updating dirstate"""
    return _merge.update(repo, node, False, True, choose)[3] > 0

def verify(repo):
    """verify the consistency of a repository"""
    return _verify.verify(repo)
