# templatekw.py - common changeset template keywords
#
# Copyright 2005-2009 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from node import hex
import encoding, patch, util, error

def showlist(templ, name, values, plural=None, **args):
    '''expand set of values.
    name is name of key in template map.
    values is list of strings or dicts.
    plural is plural of name, if not simply name + 's'.

    expansion works like this, given name 'foo'.

    if values is empty, expand 'no_foos'.

    if 'foo' not in template map, return values as a string,
    joined by space.

    expand 'start_foos'.

    for each value, expand 'foo'. if 'last_foo' in template
    map, expand it instead of 'foo' for last key.

    expand 'end_foos'.
    '''
    if plural: names = plural
    else: names = name + 's'
    if not values:
        noname = 'no_' + names
        if noname in templ:
            yield templ(noname, **args)
        return
    if name not in templ:
        if isinstance(values[0], str):
            yield ' '.join(values)
        else:
            for v in values:
                yield dict(v, **args)
        return
    startname = 'start_' + names
    if startname in templ:
        yield templ(startname, **args)
    vargs = args.copy()
    def one(v, tag=name):
        try:
            vargs.update(v)
        except (AttributeError, ValueError):
            try:
                for a, b in v:
                    vargs[a] = b
            except ValueError:
                vargs[name] = v
        return templ(tag, **vargs)
    lastname = 'last_' + name
    if lastname in templ:
        last = values.pop()
    else:
        last = None
    for v in values:
        yield one(v)
    if last is not None:
        yield one(last, tag=lastname)
    endname = 'end_' + names
    if endname in templ:
        yield templ(endname, **args)

def getfiles(repo, ctx, revcache):
    if 'files' not in revcache:
        revcache['files'] = repo.status(ctx.parents()[0].node(),
                                        ctx.node())[:3]
    return revcache['files']

def getlatesttags(repo, ctx, cache):
    '''return date, distance and name for the latest tag of rev'''

    if 'latesttags' not in cache:
        # Cache mapping from rev to a tuple with tag date, tag
        # distance and tag name
        cache['latesttags'] = {-1: (0, 0, 'null')}
    latesttags = cache['latesttags']

    rev = ctx.rev()
    todo = [rev]
    while todo:
        rev = todo.pop()
        if rev in latesttags:
            continue
        ctx = repo[rev]
        tags = [t for t in ctx.tags() if repo.tagtype(t) == 'global']
        if tags:
            latesttags[rev] = ctx.date()[0], 0, ':'.join(sorted(tags))
            continue
        try:
            # The tuples are laid out so the right one can be found by
            # comparison.
            pdate, pdist, ptag = max(
                latesttags[p.rev()] for p in ctx.parents())
        except KeyError:
            # Cache miss - recurse
            todo.append(rev)
            todo.extend(p.rev() for p in ctx.parents())
            continue
        latesttags[rev] = pdate, pdist + 1, ptag
    return latesttags[rev]

def getrenamedfn(repo, endrev=None):
    rcache = {}
    if endrev is None:
        endrev = len(repo)

    def getrenamed(fn, rev):
        '''looks up all renames for a file (up to endrev) the first
        time the file is given. It indexes on the changerev and only
        parses the manifest if linkrev != changerev.
        Returns rename info for fn at changerev rev.'''
        if fn not in rcache:
            rcache[fn] = {}
            fl = repo.file(fn)
            for i in fl:
                lr = fl.linkrev(i)
                renamed = fl.renamed(fl.node(i))
                rcache[fn][lr] = renamed
                if lr >= endrev:
                    break
        if rev in rcache[fn]:
            return rcache[fn][rev]

        # If linkrev != rev (i.e. rev not found in rcache) fallback to
        # filectx logic.
        try:
            return repo[rev][fn].renamed()
        except error.LookupError:
            return None

    return getrenamed


def showauthor(repo, ctx, templ, **args):
    return ctx.user()

def showbranches(repo, ctx, templ, **args):
    branch = ctx.branch()
    if branch != 'default':
        branch = encoding.tolocal(branch)
        return showlist(templ, 'branch', [branch], plural='branches', **args)

def showdate(repo, ctx, templ, **args):
    return ctx.date()

def showdescription(repo, ctx, templ, **args):
    return ctx.description().strip()

def showdiffstat(repo, ctx, templ, **args):
    diff = patch.diff(repo, ctx.parents()[0].node(), ctx.node())
    files, adds, removes = 0, 0, 0
    for i in patch.diffstatdata(util.iterlines(diff)):
        files += 1
        adds += i[1]
        removes += i[2]
    return '%s: +%s/-%s' % (files, adds, removes)

def showextras(repo, ctx, templ, **args):
    for key, value in sorted(ctx.extra().items()):
        args = args.copy()
        args.update(dict(key=key, value=value))
        yield templ('extra', **args)

def showfileadds(repo, ctx, templ, revcache, **args):
    return showlist(templ, 'file_add', getfiles(repo, ctx, revcache)[1], **args)

def showfilecopies(repo, ctx, templ, cache, revcache, **args):
    copies = revcache.get('copies')
    if copies is None:
        if 'getrenamed' not in cache:
            cache['getrenamed'] = getrenamedfn(repo)
        copies = []
        getrenamed = cache['getrenamed']
        for fn in ctx.files():
            rename = getrenamed(fn, ctx.rev())
            if rename:
                copies.append((fn, rename[0]))
            
    c = [{'name': x[0], 'source': x[1]} for x in copies]
    return showlist(templ, 'file_copy', c, plural='file_copies', **args)

# showfilecopiesswitch() displays file copies only if copy records are
# provided before calling the templater, usually with a --copies
# command line switch.
def showfilecopiesswitch(repo, ctx, templ, cache, revcache, **args):
    copies = revcache.get('copies') or []
    c = [{'name': x[0], 'source': x[1]} for x in copies]
    return showlist(templ, 'file_copy', c, plural='file_copies', **args)

def showfiledels(repo, ctx, templ, revcache, **args):
    return showlist(templ, 'file_del', getfiles(repo, ctx, revcache)[2], **args)

def showfilemods(repo, ctx, templ, revcache, **args):
    return showlist(templ, 'file_mod', getfiles(repo, ctx, revcache)[0], **args)

def showfiles(repo, ctx, templ, **args):
    return showlist(templ, 'file', ctx.files(), **args)

def showlatesttag(repo, ctx, templ, cache, **args):
    return getlatesttags(repo, ctx, cache)[2]

def showlatesttagdistance(repo, ctx, templ, cache, **args):
    return getlatesttags(repo, ctx, cache)[1]

def showmanifest(repo, ctx, templ, **args):
    args = args.copy()
    args.update(dict(rev=repo.manifest.rev(ctx.changeset()[0]),
                     node=hex(ctx.changeset()[0])))
    return templ('manifest', **args)

def shownode(repo, ctx, templ, **args):
    return ctx.hex()

def showrev(repo, ctx, templ, **args):
    return ctx.rev()

def showtags(repo, ctx, templ, **args):
    return showlist(templ, 'tag', ctx.tags(), **args)

keywords = {
    'author': showauthor,
    'branches': showbranches,
    'date': showdate,
    'desc': showdescription,
    'diffstat': showdiffstat,
    'extras': showextras,
    'file_adds': showfileadds,
    'file_copies': showfilecopies,
    'file_copies_switch': showfilecopiesswitch,
    'file_dels': showfiledels,
    'file_mods': showfilemods,
    'files': showfiles,
    'latesttag': showlatesttag,
    'latesttagdistance': showlatesttagdistance,
    'manifest': showmanifest,
    'node': shownode,
    'rev': showrev,
    'tags': showtags,
}

