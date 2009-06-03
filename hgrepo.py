from mercurial import localrepo, lock, node
from mercurial import changelog, dirstate, filelog, manifest, context, weakref
from mercurial.node import bin, hex, nullid, nullrev, short

from git_handler import GitHandler
from gitrepo import gitrepo


class hgrepo(localrepo.localrepository):

    def commit_import_ctx(self, wctx, ancestor, force_files = None):
    
        tr = None
        valid = 0 # don't save the dirstate if this isn't set
        try:
            force=False
            force_editor=False
            empty_ok=False
            use_dirstate=False
            update_dirstate=False
            
            commit = sorted(wctx.modified() + wctx.added())
            remove = wctx.removed()
            extra = wctx.extra().copy()
            branchname = extra['branch']
            user = wctx.user()
            text = wctx.description()

            p1, p2 = [p.node() for p in wctx.parents()]
            c1 = self.changelog.read(p1)
            c2 = self.changelog.read(p2)
            m1 = self.manifest.read(c1[0]).copy()
            m2 = self.manifest.read(c2[0])
            ma = None
            if ancestor:
                ma = ancestor.manifest()

            xp1 = hex(p1)
            if p2 == nullid: xp2 = ''
            else: xp2 = hex(p2)

            tr = self.transaction()
            trp = weakref.proxy(tr)

            # looking for files that have not actually changed content-wise,
            # but have different nodeids because they were changed and then
            # reverted, so they have changed in the revlog.
            for f in m1:
                if (f in m2) and (not f in commit) and (not m1[f] == m2[f]):
                    commit.append(f)
                    
            # check in files
            new = {}
            changed = []
            linkrev = len(self)
            for f in commit:
                self.ui.note(f + "\n")
                try:
                    fctx = wctx.filectx(f)
                    newflags = fctx.flags()
                    try:
                        new[f] = self.filecommit(fctx, m1, m2, linkrev, trp, changed)
                    except AttributeError:
                        new[f] = self._filecommit(fctx, m1, m2, linkrev, trp, changed)
                    if ((not changed or changed[-1] != f) and
                        m2.get(f) != new[f]):
                        # mention the file in the changelog if some
                        # flag changed, even if there was no content
                        # change.
                        if m1.flags(f) != newflags:
                            changed.append(f)
                    m1.set(f, newflags)

                except (OSError, IOError):
                    remove.append(f)
            
            updated, added = [], []
            for f in sorted(changed):
                if f in m1 or f in m2:
                    updated.append(f)
                else:
                    added.append(f)
            
            # update manifest
            m1.update(new)
            removed = [f for f in sorted(remove) if f in m1 or f in m2]
            removed1 = []

            for f in removed:
                if f in m1:
                    del m1[f]
                    removed1.append(f)
                else:
                    if ma and (f in ma):
                        del ma[f]
                        removed.remove(f)
            
            mn = self.manifest.add(m1, trp, linkrev, c1[0], c2[0],
                                   (new, removed1))

            #lines = [line.rstrip() for line in text.rstrip().splitlines()]
            #while lines and not lines[0]:
            #    del lines[0]
            #text = '\n'.join(lines)
            if text[-1] == "\n":
                text = text[:-1]
            
            file_list = []
            if force_files == False:
                file_list = []
            else:
                if force_files and len(force_files) > 0:
                    file_list = force_files
                else:
                    file_list = changed + removed
            
            self.changelog.delayupdate()
            n = self.changelog.add(mn, file_list, text, trp, p1, p2,
                                   user, wctx.date(), extra)
            p = lambda: self.changelog.writepending() and self.root or ""
            self.hook('pretxncommit', throw=True, node=hex(n), parent1=xp1,
                      parent2=xp2, pending=p)
            self.changelog.finalize(trp)
            tr.close()
            
            if self.branchcache:
                self.branchtags()

            if update_dirstate:
                self.dirstate.setparents(n)
            valid = 1 # our dirstate updates are complete

            self.hook("commit", node=hex(n), parent1=xp1, parent2=xp2)
            return n
        finally:
            if not valid: # don't save our updated dirstate
                self.dirstate.invalidate()
            del tr

    def clone(self, remote, heads=[], stream=False):
        if isinstance(remote, gitrepo):
            git = GitHandler(self, self.ui)
            git.remote_add('origin', remote.path)

        super(hgrepo, self).clone(remote, heads)

    def pull(self, remote, heads=None, force=False):
        if isinstance(remote, gitrepo):
            git = GitHandler(self, self.ui)
            git.fetch(remote.path)
        else:
            super(hgrepo, self).pull(remote, heads, force)

    def push(self, remote, force=False, revs=None):
        if isinstance(remote, gitrepo):
            git = GitHandler(self, self.ui)
            git.push(remote.path)
        else:
            super(hgrepo, self).push(remote, force, revs)

instance = hgrepo
