import os
import stat
import re
import errno

from mercurial import dirstate
try:
    from mercurial import ignore
    ignore.readpats
    ignoremod = True
except:
    # ignore module was removed in Mercurial 3.5
    ignoremod = False
from mercurial import match as matchmod
from mercurial import osutil
from mercurial import scmutil
# pathauditor moved to pathutil in 2.8
try:
    from mercurial import pathutil
    pathutil.pathauditor
except:
    pathutil = scmutil
from mercurial import util
from mercurial.i18n import _

def gignorepats(orig, lines, root=None):
    '''parse lines (iterable) of .gitignore text, returning a tuple of
    (patterns, parse errors). These patterns should be given to compile()
    to be validated and converted into a match function.'''
    syntaxes = {'re': 'relre:', 'regexp': 'relre:', 'glob': 'relglob:'}
    syntax = 'glob:'

    patterns = []
    warnings = []

    for line in lines:
        if "#" in line:
            _commentre = re.compile(r'((^|[^\\])(\\\\)*)#.*')
            # remove comments prefixed by an even number of escapes
            line = _commentre.sub(r'\1', line)
            # fixup properly escaped comments that survived the above
            line = line.replace("\\#", "#")
        line = line.rstrip()
        if not line:
            continue

        if line.startswith('!'):
            warnings.append(_("unsupported ignore pattern '%s'") % line)
            continue
        if re.match(r'(:?.*/)?\.hg(:?/|$)', line):
            continue
        rootprefix = '%s/' % root if root else ''
        if line.startswith('/'):
            line = line[1:]
            rootsuffixes = ['']
        else:
            rootsuffixes = ['', '**/']
        for rootsuffix in rootsuffixes:
            pat = syntax + rootprefix + rootsuffix + line
            for s, rels in syntaxes.iteritems():
                if line.startswith(rels):
                    pat = line
                    break
                elif line.startswith(s + ':'):
                    pat = rels + line[len(s) + 1:]
                    break
            patterns.append(pat)

    return patterns, warnings

def gignore(root, files, warn, extrapatterns=None):
    allpats = []
    pats = []
    if ignoremod:
        pats = ignore.readpats(root, files, warn)
        for f, patlist in pats:
            allpats.extend(patlist)
    else:
        allpats.extend(['include:%s' % f for f in files])

    if extrapatterns:
        allpats.extend(extrapatterns)
    if not allpats:
        return util.never
    try:
        ignorefunc = matchmod.match(root, '', [], allpats)
    except util.Abort:
        for f, patlist in pats:
            try:
                matchmod.match(root, '', [], patlist)
            except util.Abort, inst:
                raise util.Abort('%s: %s' % (f, inst[0]))
        if extrapatterns:
            try:
                matchmod.match(root, '', [], extrapatterns)
            except util.Abort, inst:
                raise util.Abort('%s: %s' % ('extra patterns', inst[0]))
    return ignorefunc

class gitdirstate(dirstate.dirstate):
    @dirstate.rootcache('.hgignore')
    def _ignore(self):
        files = [self._join('.hgignore')]
        for name, path in self._ui.configitems("ui"):
            if name == 'ignore' or name.startswith('ignore.'):
                files.append(util.expandpath(path))
        patterns = []
        # Only use .gitignore if there's no .hgignore
        try:
            fp = open(files[0])
            fp.close()
        except:
            fns = self._finddotgitignores()
            for fn in fns:
                d = os.path.dirname(fn)
                fn = self.pathto(fn)
                if not os.path.exists(fn):
                    continue
                fp = open(fn)
                pats, warnings = gignorepats(None, fp, root=d)
                for warning in warnings:
                    self._ui.warn("%s: %s\n" % (fn, warning))
                patterns.extend(pats)
        return gignore(self._root, files, self._ui.warn,
                             extrapatterns=patterns)

    def _finddotgitignores(self):
        """A copy of dirstate.walk. This is called from the new _ignore method,
        which is called by dirstate.walk, which would cause infinite recursion,
        except _finddotgitignores calls the superclass _ignore directly."""
        match = matchmod.match(self._root, self.getcwd(),
                               ['relglob:.gitignore'])
        # TODO: need subrepos?
        subrepos = []
        unknown = True
        ignored = False

        def fwarn(f, msg):
            self._ui.warn('%s: %s\n' % (self.pathto(f), msg))
            return False

        ignore = super(gitdirstate, self)._ignore
        dirignore = self._dirignore
        if ignored:
            ignore = util.never
            dirignore = util.never
        elif not unknown:
            # if unknown and ignored are False, skip step 2
            ignore = util.always
            dirignore = util.always

        matchfn = match.matchfn
        matchalways = match.always()
        matchtdir = match.traversedir
        dmap = self._map
        listdir = osutil.listdir
        lstat = os.lstat
        dirkind = stat.S_IFDIR
        regkind = stat.S_IFREG
        lnkkind = stat.S_IFLNK
        join = self._join

        exact = skipstep3 = False
        if matchfn == match.exact:  # match.exact
            exact = True
            dirignore = util.always                  # skip step 2
        elif match.files() and not match.anypats():  # match.match, no patterns
            skipstep3 = True

        if not exact and self._checkcase:
            normalize = self._normalize
            skipstep3 = False
        else:
            normalize = None

        # step 1: find all explicit files
        results, work, dirsnotfound = self._walkexplicit(match, subrepos)

        skipstep3 = skipstep3 and not (work or dirsnotfound)
        if work and isinstance(work[0], tuple):
            # Mercurial >= 3.3.3
            work = [nd for nd, d in work if not dirignore(d)]
        else:
            work = [d for d in work if not dirignore(d)]
        wadd = work.append

        # step 2: visit subdirectories
        while work:
            nd = work.pop()
            skip = None
            if nd == '.':
                nd = ''
            else:
                skip = '.hg'
            try:
                entries = listdir(join(nd), stat=True, skip=skip)
            except OSError, inst:
                if inst.errno in (errno.EACCES, errno.ENOENT):
                    fwarn(nd, inst.strerror)
                    continue
                raise
            for f, kind, st in entries:
                if normalize:
                    nf = normalize(nd and (nd + "/" + f) or f, True, True)
                else:
                    nf = nd and (nd + "/" + f) or f
                if nf not in results:
                    if kind == dirkind:
                        if not ignore(nf):
                            if matchtdir:
                                matchtdir(nf)
                            wadd(nf)
                        if nf in dmap and (matchalways or matchfn(nf)):
                            results[nf] = None
                    elif kind == regkind or kind == lnkkind:
                        if nf in dmap:
                            if matchalways or matchfn(nf):
                                results[nf] = st
                        elif (matchalways or matchfn(nf)) and not ignore(nf):
                            results[nf] = st
                    elif nf in dmap and (matchalways or matchfn(nf)):
                        results[nf] = None

        for s in subrepos:
            del results[s]
        del results['.hg']

        # step 3: report unseen items in the dmap hash
        if not skipstep3 and not exact:
            if not results and matchalways:
                visit = dmap.keys()
            else:
                visit = [f for f in dmap if f not in results and matchfn(f)]
            visit.sort()

            if unknown:
                # unknown == True means we walked the full directory tree
                # above. So if a file is not seen it was either a) not matching
                # matchfn b) ignored, c) missing, or d) under a symlink
                # directory.
                audit_path = pathutil.pathauditor(self._root)

                for nf in iter(visit):
                    # Report ignored items in the dmap as long as they are not
                    # under a symlink directory.
                    if audit_path.check(nf):
                        try:
                            results[nf] = lstat(join(nf))
                        except OSError:
                            # file doesn't exist
                            results[nf] = None
                    else:
                        # It's either missing or under a symlink directory
                        results[nf] = None
            else:
                # We may not have walked the full directory tree above,
                # so stat everything we missed.
                nf = iter(visit).next
                for st in util.statfiles([join(i) for i in visit]):
                    results[nf()] = st
        return results.keys()
