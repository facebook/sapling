# cmdutil.py - help for command processing in mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
from node import *
from i18n import gettext as _
demandload(globals(), 'mdiff util')
demandload(globals(), 'os sys')

revrangesep = ':'

def revfix(repo, val, defval):
    '''turn user-level id of changeset into rev number.
    user-level id can be tag, changeset, rev number, or negative rev
    number relative to number of revs (-1 is tip, etc).'''
    if not val:
        return defval
    try:
        num = int(val)
        if str(num) != val:
            raise ValueError
        if num < 0:
            num += repo.changelog.count()
        if num < 0:
            num = 0
        elif num >= repo.changelog.count():
            raise ValueError
    except ValueError:
        try:
            num = repo.changelog.rev(repo.lookup(val))
        except KeyError:
            raise util.Abort(_('invalid revision identifier %s') % val)
    return num

def revpair(ui, repo, revs):
    '''return pair of nodes, given list of revisions. second item can
    be None, meaning use working dir.'''
    if not revs:
        return repo.dirstate.parents()[0], None
    end = None
    if len(revs) == 1:
        start = revs[0]
        if revrangesep in start:
            start, end = start.split(revrangesep, 1)
            start = revfix(repo, start, 0)
            end = revfix(repo, end, repo.changelog.count() - 1)
        else:
            start = revfix(repo, start, None)
    elif len(revs) == 2:
        if revrangesep in revs[0] or revrangesep in revs[1]:
            raise util.Abort(_('too many revisions specified'))
        start = revfix(repo, revs[0], None)
        end = revfix(repo, revs[1], None)
    else:
        raise util.Abort(_('too many revisions specified'))
    if end is not None: end = repo.lookup(end)
    return repo.lookup(start), end

def revrange(ui, repo, revs):
    """Yield revision as strings from a list of revision specifications."""
    seen = {}
    for spec in revs:
        if revrangesep in spec:
            start, end = spec.split(revrangesep, 1)
            start = revfix(repo, start, 0)
            end = revfix(repo, end, repo.changelog.count() - 1)
            step = start > end and -1 or 1
            for rev in xrange(start, end+step, step):
                if rev in seen:
                    continue
                seen[rev] = 1
                yield str(rev)
        else:
            rev = revfix(repo, spec, None)
            if rev in seen:
                continue
            seen[rev] = 1
            yield str(rev)

def make_filename(repo, pat, node,
                  total=None, seqno=None, revwidth=None, pathname=None):
    node_expander = {
        'H': lambda: hex(node),
        'R': lambda: str(repo.changelog.rev(node)),
        'h': lambda: short(node),
        }
    expander = {
        '%': lambda: '%',
        'b': lambda: os.path.basename(repo.root),
        }

    try:
        if node:
            expander.update(node_expander)
        if node and revwidth is not None:
            expander['r'] = (lambda:
                    str(repo.changelog.rev(node)).zfill(revwidth))
        if total is not None:
            expander['N'] = lambda: str(total)
        if seqno is not None:
            expander['n'] = lambda: str(seqno)
        if total is not None and seqno is not None:
            expander['n'] = lambda:str(seqno).zfill(len(str(total)))
        if pathname is not None:
            expander['s'] = lambda: os.path.basename(pathname)
            expander['d'] = lambda: os.path.dirname(pathname) or '.'
            expander['p'] = lambda: pathname

        newname = []
        patlen = len(pat)
        i = 0
        while i < patlen:
            c = pat[i]
            if c == '%':
                i += 1
                c = pat[i]
                c = expander[c]()
            newname.append(c)
            i += 1
        return ''.join(newname)
    except KeyError, inst:
        raise util.Abort(_("invalid format spec '%%%s' in output file name") %
                         inst.args[0])

def make_file(repo, pat, node=None,
              total=None, seqno=None, revwidth=None, mode='wb', pathname=None):
    if not pat or pat == '-':
        return 'w' in mode and sys.stdout or sys.stdin
    if hasattr(pat, 'write') and 'w' in mode:
        return pat
    if hasattr(pat, 'read') and 'r' in mode:
        return pat
    return open(make_filename(repo, pat, node, total, seqno, revwidth,
                              pathname),
                mode)

def matchpats(repo, pats=[], opts={}, head=''):
    cwd = repo.getcwd()
    if not pats and cwd:
        opts['include'] = [os.path.join(cwd, i)
                           for i in opts.get('include', [])]
        opts['exclude'] = [os.path.join(cwd, x)
                           for x in opts.get('exclude', [])]
        cwd = ''
    return util.cmdmatcher(repo.root, cwd, pats or ['.'], opts.get('include'),
                           opts.get('exclude'), head)

def makewalk(repo, pats=[], opts={}, node=None, head='', badmatch=None):
    files, matchfn, anypats = matchpats(repo, pats, opts, head)
    exact = dict(zip(files, files))
    def walk():
        for src, fn in repo.walk(node=node, files=files, match=matchfn,
                                 badmatch=badmatch):
            yield src, fn, util.pathto(repo.getcwd(), fn), fn in exact
    return files, matchfn, walk()

def walk(repo, pats=[], opts={}, node=None, head='', badmatch=None):
    files, matchfn, results = makewalk(repo, pats, opts, node, head, badmatch)
    for r in results:
        yield r

def findrenames(repo, added=None, removed=None, threshold=0.5):
    if added is None or removed is None:
        added, removed = repo.status()[1:3]
    changes = repo.changelog.read(repo.dirstate.parents()[0])
    mf = repo.manifest.read(changes[0])
    for a in added:
        aa = repo.wread(a)
        bestscore, bestname = None, None
        for r in removed:
            rr = repo.file(r).read(mf[r])
            delta = mdiff.textdiff(aa, rr)
            if len(delta) < len(aa):
                myscore = 1.0 - (float(len(delta)) / len(aa))
                if bestscore is None or myscore > bestscore:
                    bestscore, bestname = myscore, r
        if bestname and bestscore >= threshold:
            yield bestname, a, bestscore

def addremove(repo, pats=[], opts={}, wlock=None, dry_run=None,
              similarity=None):
    if dry_run is None:
        dry_run = opts.get('dry_run')
    if similarity is None:
        similarity = float(opts.get('similarity') or 0)
    add, remove = [], []
    mapping = {}
    for src, abs, rel, exact in walk(repo, pats, opts):
        if src == 'f' and repo.dirstate.state(abs) == '?':
            add.append(abs)
            mapping[abs] = rel, exact
            if repo.ui.verbose or not exact:
                repo.ui.status(_('adding %s\n') % ((pats and rel) or abs))
        if repo.dirstate.state(abs) != 'r' and not os.path.exists(rel):
            remove.append(abs)
            mapping[abs] = rel, exact
            if repo.ui.verbose or not exact:
                repo.ui.status(_('removing %s\n') % ((pats and rel) or abs))
    if not dry_run:
        repo.add(add, wlock=wlock)
        repo.remove(remove, wlock=wlock)
    if similarity > 0:
        for old, new, score in findrenames(repo, add, remove, similarity):
            oldrel, oldexact = mapping[old]
            newrel, newexact = mapping[new]
            if repo.ui.verbose or not oldexact or not newexact:
                repo.ui.status(_('recording removal of %s as rename to %s '
                                 '(%d%% similar)\n') %
                               (oldrel, newrel, score * 100))
            if not dry_run:
                repo.copy(old, new, wlock=wlock)
