# commands.py - command processing for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
from node import *
from i18n import gettext as _
demandload(globals(), 'util')
demandload(globals(), 'os sys')

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
        raise util.Abort(_("invalid format spec '%%%s' in output file name"),
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
        opts['include'] = [os.path.join(cwd, i) for i in opts['include']]
        opts['exclude'] = [os.path.join(cwd, x) for x in opts['exclude']]
        cwd = ''
    return util.cmdmatcher(repo.root, cwd, pats or ['.'], opts.get('include'),
                           opts.get('exclude'), head)

def makewalk(repo, pats, opts, node=None, head='', badmatch=None):
    files, matchfn, anypats = matchpats(repo, pats, opts, head)
    exact = dict(zip(files, files))
    def walk():
        for src, fn in repo.walk(node=node, files=files, match=matchfn,
                                 badmatch=badmatch):
            yield src, fn, util.pathto(repo.getcwd(), fn), fn in exact
    return files, matchfn, walk()

def walk(repo, pats, opts, node=None, head='', badmatch=None):
    files, matchfn, results = makewalk(repo, pats, opts, node, head, badmatch)
    for r in results:
        yield r

def addremove(repo, pats, opts={}, wlock=None, dry_run=None):
    if dry_run is None:
        dry_run = opts.get('dry_run')
    add, remove = [], []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if src == 'f' and repo.dirstate.state(abs) == '?':
            add.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('adding %s\n') % ((pats and rel) or abs))
        if repo.dirstate.state(abs) != 'r' and not os.path.exists(rel):
            remove.append(abs)
            if repo.ui.verbose or not exact:
                repo.ui.status(_('removing %s\n') % ((pats and rel) or abs))
    if not dry_run:
        repo.add(add, wlock=wlock)
        repo.remove(remove, wlock=wlock)
