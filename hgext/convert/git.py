# git.py - git support for the convert extension
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import subprocess
from mercurial import util, config
from mercurial.node import hex, nullid
from mercurial.i18n import _

from common import NoRepo, commit, converter_source, checktool

class submodule(object):
    def __init__(self, path, node, url):
        self.path = path
        self.node = node
        self.url = url

    def hgsub(self):
        return "%s = [git]%s" % (self.path, self.url)

    def hgsubstate(self):
        return "%s %s" % (self.node, self.path)

class convert_git(converter_source):
    # Windows does not support GIT_DIR= construct while other systems
    # cannot remove environment variable. Just assume none have
    # both issues.
    if util.safehasattr(os, 'unsetenv'):
        def gitopen(self, s, err=None):
            prevgitdir = os.environ.get('GIT_DIR')
            os.environ['GIT_DIR'] = self.path
            try:
                if err == subprocess.PIPE:
                    (stdin, stdout, stderr) = util.popen3(s)
                    return stdout
                elif err == subprocess.STDOUT:
                    return self.popen_with_stderr(s)
                else:
                    return util.popen(s, 'rb')
            finally:
                if prevgitdir is None:
                    del os.environ['GIT_DIR']
                else:
                    os.environ['GIT_DIR'] = prevgitdir

        def gitpipe(self, s):
            prevgitdir = os.environ.get('GIT_DIR')
            os.environ['GIT_DIR'] = self.path
            try:
                return util.popen3(s)
            finally:
                if prevgitdir is None:
                    del os.environ['GIT_DIR']
                else:
                    os.environ['GIT_DIR'] = prevgitdir

    else:
        def gitopen(self, s, err=None):
            if err == subprocess.PIPE:
                (sin, so, se) = util.popen3('GIT_DIR=%s %s' % (self.path, s))
                return so
            elif err == subprocess.STDOUT:
                    return self.popen_with_stderr(s)
            else:
                return util.popen('GIT_DIR=%s %s' % (self.path, s), 'rb')

        def gitpipe(self, s):
            return util.popen3('GIT_DIR=%s %s' % (self.path, s))

    def popen_with_stderr(self, s):
        p = subprocess.Popen(s, shell=True, bufsize=-1,
                             close_fds=util.closefds,
                             stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT,
                             universal_newlines=False,
                             env=None)
        return p.stdout

    def gitread(self, s):
        fh = self.gitopen(s)
        data = fh.read()
        return data, fh.close()

    def __init__(self, ui, path, rev=None):
        super(convert_git, self).__init__(ui, path, rev=rev)

        if os.path.isdir(path + "/.git"):
            path += "/.git"
        if not os.path.exists(path + "/objects"):
            raise NoRepo(_("%s does not look like a Git repository") % path)

        # The default value (50) is based on the default for 'git diff'.
        similarity = ui.configint('convert', 'git.similarity', default=50)
        if similarity < 0 or similarity > 100:
            raise util.Abort(_('similarity must be between 0 and 100'))
        if similarity > 0:
            self.simopt = '-C%d%%' % similarity
            findcopiesharder = ui.configbool('convert', 'git.findcopiesharder',
                                             False)
            if findcopiesharder:
                self.simopt += ' --find-copies-harder'
        else:
            self.simopt = ''

        checktool('git', 'git')

        self.path = path
        self.submodules = []

        self.catfilepipe = self.gitpipe('git cat-file --batch')

    def after(self):
        for f in self.catfilepipe:
            f.close()

    def getheads(self):
        if not self.rev:
            heads, ret = self.gitread('git rev-parse --branches --remotes')
            heads = heads.splitlines()
        else:
            heads, ret = self.gitread("git rev-parse --verify %s" % self.rev)
            heads = [heads[:-1]]
        if ret:
            raise util.Abort(_('cannot retrieve git heads'))
        return heads

    def catfile(self, rev, type):
        if rev == hex(nullid):
            raise IOError
        self.catfilepipe[0].write(rev+'\n')
        self.catfilepipe[0].flush()
        info = self.catfilepipe[1].readline().split()
        if info[1] != type:
            raise util.Abort(_('cannot read %r object at %s') % (type, rev))
        size = int(info[2])
        data = self.catfilepipe[1].read(size)
        if len(data) < size:
            raise util.Abort(_('cannot read %r object at %s: unexpected size')
                             % (type, rev))
        # read the trailing newline
        self.catfilepipe[1].read(1)
        return data

    def getfile(self, name, rev):
        if rev == hex(nullid):
            return None, None
        if name == '.hgsub':
            data = '\n'.join([m.hgsub() for m in self.submoditer()])
            mode = ''
        elif name == '.hgsubstate':
            data = '\n'.join([m.hgsubstate() for m in self.submoditer()])
            mode = ''
        else:
            data = self.catfile(rev, "blob")
            mode = self.modecache[(name, rev)]
        return data, mode

    def submoditer(self):
        null = hex(nullid)
        for m in sorted(self.submodules, key=lambda p: p.path):
            if m.node != null:
                yield m

    def parsegitmodules(self, content):
        """Parse the formatted .gitmodules file, example file format:
        [submodule "sub"]\n
        \tpath = sub\n
        \turl = git://giturl\n
        """
        self.submodules = []
        c = config.config()
        # Each item in .gitmodules starts with whitespace that cant be parsed
        c.parse('.gitmodules', '\n'.join(line.strip() for line in
                               content.split('\n')))
        for sec in c.sections():
            s = c[sec]
            if 'url' in s and 'path' in s:
                self.submodules.append(submodule(s['path'], '', s['url']))

    def retrievegitmodules(self, version):
        modules, ret = self.gitread("git show %s:%s" % (version, '.gitmodules'))
        if ret:
            raise util.Abort(_('cannot read submodules config file in %s') %
                             version)
        self.parsegitmodules(modules)
        for m in self.submodules:
            node, ret = self.gitread("git rev-parse %s:%s" % (version, m.path))
            if ret:
                continue
            m.node = node.strip()

    def getchanges(self, version, full):
        if full:
            raise util.Abort(_("convert from git do not support --full"))
        self.modecache = {}
        fh = self.gitopen("git diff-tree -z --root -m -r %s %s" % (
            self.simopt, version))
        changes = []
        copies = {}
        seen = set()
        entry = None
        subexists = [False]
        subdeleted = [False]
        difftree = fh.read().split('\x00')
        lcount = len(difftree)
        i = 0

        def add(entry, f, isdest):
            seen.add(f)
            h = entry[3]
            p = (entry[1] == "100755")
            s = (entry[1] == "120000")
            renamesource = (not isdest and entry[4][0] == 'R')

            if f == '.gitmodules':
                subexists[0] = True
                if entry[4] == 'D' or renamesource:
                    subdeleted[0] = True
                    changes.append(('.hgsub', hex(nullid)))
                else:
                    changes.append(('.hgsub', ''))
            elif entry[1] == '160000' or entry[0] == ':160000':
                subexists[0] = True
            else:
                if renamesource:
                    h = hex(nullid)
                self.modecache[(f, h)] = (p and "x") or (s and "l") or ""
                changes.append((f, h))

        while i < lcount:
            l = difftree[i]
            i += 1
            if not entry:
                if not l.startswith(':'):
                    continue
                entry = l.split()
                continue
            f = l
            if f not in seen:
                add(entry, f, False)
            # A file can be copied multiple times, or modified and copied
            # simultaneously. So f can be repeated even if fdest isn't.
            if entry[4][0] in 'RC':
                # rename or copy: next line is the destination
                fdest = difftree[i]
                i += 1
                if fdest not in seen:
                    add(entry, fdest, True)
                    # .gitmodules isn't imported at all, so it being copied to
                    # and fro doesn't really make sense
                    if f != '.gitmodules' and fdest != '.gitmodules':
                        copies[fdest] = f
            entry = None
        if fh.close():
            raise util.Abort(_('cannot read changes in %s') % version)

        if subexists[0]:
            if subdeleted[0]:
                changes.append(('.hgsubstate', hex(nullid)))
            else:
                self.retrievegitmodules(version)
                changes.append(('.hgsubstate', ''))
        return (changes, copies, set())

    def getcommit(self, version):
        c = self.catfile(version, "commit") # read the commit hash
        end = c.find("\n\n")
        message = c[end + 2:]
        message = self.recode(message)
        l = c[:end].splitlines()
        parents = []
        author = committer = None
        for e in l[1:]:
            n, v = e.split(" ", 1)
            if n == "author":
                p = v.split()
                tm, tz = p[-2:]
                author = " ".join(p[:-2])
                if author[0] == "<": author = author[1:-1]
                author = self.recode(author)
            if n == "committer":
                p = v.split()
                tm, tz = p[-2:]
                committer = " ".join(p[:-2])
                if committer[0] == "<": committer = committer[1:-1]
                committer = self.recode(committer)
            if n == "parent":
                parents.append(v)

        if committer and committer != author:
            message += "\ncommitter: %s\n" % committer
        tzs, tzh, tzm = tz[-5:-4] + "1", tz[-4:-2], tz[-2:]
        tz = -int(tzs) * (int(tzh) * 3600 + int(tzm))
        date = tm + " " + str(tz)

        c = commit(parents=parents, date=date, author=author, desc=message,
                   rev=version)
        return c

    def numcommits(self):
        return len([None for _ in self.gitopen('git rev-list --all')])

    def gettags(self):
        tags = {}
        alltags = {}
        fh = self.gitopen('git ls-remote --tags "%s"' % self.path,
                          err=subprocess.STDOUT)
        prefix = 'refs/tags/'

        # Build complete list of tags, both annotated and bare ones
        for line in fh:
            line = line.strip()
            if line.startswith("error:") or line.startswith("fatal:"):
                raise util.Abort(_('cannot read tags from %s') % self.path)
            node, tag = line.split(None, 1)
            if not tag.startswith(prefix):
                continue
            alltags[tag[len(prefix):]] = node
        if fh.close():
            raise util.Abort(_('cannot read tags from %s') % self.path)

        # Filter out tag objects for annotated tag refs
        for tag in alltags:
            if tag.endswith('^{}'):
                tags[tag[:-3]] = alltags[tag]
            else:
                if tag + '^{}' in alltags:
                    continue
                else:
                    tags[tag] = alltags[tag]

        return tags

    def getchangedfiles(self, version, i):
        changes = []
        if i is None:
            fh = self.gitopen("git diff-tree --root -m -r %s" % version)
            for l in fh:
                if "\t" not in l:
                    continue
                m, f = l[:-1].split("\t")
                changes.append(f)
        else:
            fh = self.gitopen('git diff-tree --name-only --root -r %s '
                              '"%s^%s" --' % (version, version, i + 1))
            changes = [f.rstrip('\n') for f in fh]
        if fh.close():
            raise util.Abort(_('cannot read changes in %s') % version)

        return changes

    def getbookmarks(self):
        bookmarks = {}

        # Interesting references in git are prefixed
        prefix = 'refs/heads/'
        prefixlen = len(prefix)

        # factor two commands
        gitcmd = { 'remote/': 'git ls-remote --heads origin',
                          '': 'git show-ref'}

        # Origin heads
        for reftype in gitcmd:
            try:
                fh = self.gitopen(gitcmd[reftype], err=subprocess.PIPE)
                for line in fh:
                    line = line.strip()
                    rev, name = line.split(None, 1)
                    if not name.startswith(prefix):
                        continue
                    name = '%s%s' % (reftype, name[prefixlen:])
                    bookmarks[name] = rev
            except Exception:
                pass

        return bookmarks

    def checkrevformat(self, revstr, mapname='splicemap'):
        """ git revision string is a 40 byte hex """
        self.checkhexformat(revstr, mapname)
