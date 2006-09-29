# hg.py - repository classes for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from repo import *
from demandload import *
from i18n import gettext as _
demandload(globals(), "localrepo bundlerepo httprepo sshrepo statichttprepo")
demandload(globals(), "errno lock os shutil util merge@_merge verify@_verify")

def _local(path):
    return (os.path.isfile(util.drop_scheme('file', path)) and
            bundlerepo or localrepo)

schemes = {
    'bundle': bundlerepo,
    'file': _local,
    'hg': httprepo,
    'http': httprepo,
    'https': httprepo,
    'old-http': statichttprepo,
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

repo_setup_hooks = []

def repository(ui, path='', create=False):
    """return a repository object for the specified path"""
    repo = _lookup(path).instance(ui, path, create)
    for hook in repo_setup_hooks:
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
        src_repo = repository(ui, source)
    else:
        src_repo = source
        source = src_repo.url()

    if dest is None:
        dest = defaultdest(source)

    def localpath(path):
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

    dest_repo = repository(ui, dest, create=True)

    dest_path = None
    dir_cleanup = None
    if dest_repo.local():
        dest_path = os.path.realpath(dest_repo.root)
        dir_cleanup = DirCleanup(dest_path)

    abspath = source
    copy = False
    if src_repo.local() and dest_repo.local():
        abspath = os.path.abspath(source)
        copy = not pull and not rev

    src_lock, dest_lock = None, None
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
        # we lock here to avoid premature writing to the target
        dest_lock = lock.lock(os.path.join(dest_path, ".hg", "lock"))

        # we need to remove the (empty) data dir in dest so copyfiles
        # can do its work
        os.rmdir(os.path.join(dest_path, ".hg", "data"))
        files = "data 00manifest.d 00manifest.i 00changelog.d 00changelog.i"
        for f in files.split():
            src = os.path.join(source, ".hg", f)
            dst = os.path.join(dest_path, ".hg", f)
            try:
                util.copyfiles(src, dst)
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise

        # we need to re-init the repo after manually copying the data
        # into it
        dest_repo = repository(ui, dest)

    else:
        revs = None
        if rev:
            if not src_repo.local():
                raise util.Abort(_("clone by revision not supported yet "
                                   "for remote repositories"))
            revs = [src_repo.lookup(r) for r in rev]

        if dest_repo.local():
            dest_repo.clone(src_repo, heads=revs, stream=stream)
        elif src_repo.local():
            src_repo.push(dest_repo, revs=revs)
        else:
            raise util.Abort(_("clone from remote to remote not supported"))

    if src_lock:
        src_lock.release()

    if dest_repo.local():
        fp = dest_repo.opener("hgrc", "w", text=True)
        fp.write("[paths]\n")
        fp.write("default = %s\n" % abspath)
        fp.close()

        if dest_lock:
            dest_lock.release()

        if update:
            _merge.update(dest_repo, dest_repo.changelog.tip())
    if dir_cleanup:
        dir_cleanup.close()

    return src_repo, dest_repo

def update(repo, node):
    """update the working directory to node, merging linear changes"""
    return _merge.update(repo, node)

def clean(repo, node, wlock=None, show_stats=True):
    """forcibly switch the working directory to node, clobbering changes"""
    return _merge.update(repo, node, force=True, wlock=wlock,
                         show_stats=show_stats)

def merge(repo, node, force=None, remind=True, wlock=None):
    """branch merge with node, resolving changes"""
    return _merge.update(repo, node, branchmerge=True, force=force,
                         remind=remind, wlock=wlock)

def revert(repo, node, choose, wlock):
    """revert changes to revision in node without updating dirstate"""
    return _merge.update(repo, node, force=True, partial=choose,
                         show_stats=False, wlock=wlock)

def verify(repo):
    """verify the consistency of a repository"""
    return _verify.verify(repo)
