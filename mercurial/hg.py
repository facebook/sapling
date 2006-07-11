# hg.py - repository classes for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from repo import *
from demandload import *
from i18n import gettext as _
demandload(globals(), "localrepo bundlerepo httprepo sshrepo statichttprepo")
demandload(globals(), "errno lock os shutil util")

def bundle(ui, path):
    if path.startswith('bundle://'):
        path = path[9:]
    else:
        path = path[7:]
    s = path.split("+", 1)
    if len(s) == 1:
        repopath, bundlename = "", s[0]
    else:
        repopath, bundlename = s
    return bundlerepo.bundlerepository(ui, repopath, bundlename)

def hg(ui, path):
    ui.warn(_("hg:// syntax is deprecated, please use http:// instead\n"))
    return httprepo.httprepository(ui, path.replace("hg://", "http://"))

def local_(ui, path, create=0):
    if path.startswith('file:'):
        path = path[5:]
    return localrepo.localrepository(ui, path, create)

def ssh_(ui, path, create=0):
    return sshrepo.sshrepository(ui, path, create)

def old_http(ui, path):
    ui.warn(_("old-http:// syntax is deprecated, "
              "please use static-http:// instead\n"))
    return statichttprepo.statichttprepository(
        ui, path.replace("old-http://", "http://"))

def static_http(ui, path):
    return statichttprepo.statichttprepository(
        ui, path.replace("static-http://", "http://"))

schemes = {
    'bundle': bundle,
    'file': local_,
    'hg': hg,
    'http': lambda ui, path: httprepo.httprepository(ui, path),
    'https': lambda ui, path: httprepo.httpsrepository(ui, path),
    'old-http': old_http,
    'ssh': ssh_,
    'static-http': static_http,
    }

def repository(ui, path=None, create=0):
    scheme = None
    if path:
        c = path.find(':')
        if c > 0:
            scheme = schemes.get(path[:c])
    else:
        path = ''
    ctor = scheme or schemes['file']
    if create:
        try:
            return ctor(ui, path, create)
        except TypeError:
            raise util.Abort(_('cannot create new repository over "%s" protocol') %
                             scheme)
    return ctor(ui, path)

def clone(ui, source, dest=None, pull=False, rev=None, update=True):
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
    
    Keyword arguments:

    dest: URL of destination repository to create (defaults to base
    name of source repository)

    pull: always pull from source repository, even in local case

    rev: revision to clone up to (implies pull=True)

    update: update working directory after clone completes, if
    destination is local repository
    """
    if dest is None:
        dest = os.path.basename(os.path.normpath(source))

    if os.path.exists(dest):
        raise util.Abort(_("destination '%s' already exists"), dest)

    class DirCleanup(object):
        def __init__(self, dir_):
            self.rmtree = shutil.rmtree
            self.dir_ = dir_
        def close(self):
            self.dir_ = None
        def __del__(self):
            if self.dir_:
                self.rmtree(self.dir_, True)

    src_repo = repository(ui, source)

    dest_repo = None
    try:
        dest_repo = repository(ui, dest)
        raise util.Abort(_("destination '%s' already exists." % dest))
    except RepoError:
        dest_repo = repository(ui, dest, create=True)

    dest_path = None
    dir_cleanup = None
    if dest_repo.local():
        dest_path = os.path.realpath(dest)
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
            dest_repo.pull(src_repo, heads=revs)
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
            dest_repo.update(dest_repo.changelog.tip())
    if dir_cleanup:
        dir_cleanup.close()

    return src_repo, dest_repo
