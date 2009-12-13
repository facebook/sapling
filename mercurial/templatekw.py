# templatekw.py - common changeset template keywords
#
# Copyright 2005-2009 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import encoding

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

def showauthor(ctx, templ, **args):
    return ctx.user()

def showbranches(ctx, templ, **args):
    branch = ctx.branch()
    if branch != 'default':
        branch = encoding.tolocal(branch)
        return showlist(templ, 'branch', [branch], plural='branches', **args)

def showdate(ctx, templ, **args):
    return ctx.date()

def showdescription(ctx, templ, **args):
    return ctx.description().strip()

def showextras(ctx, templ, **args):
    for key, value in sorted(ctx.extra().items()):
        args = args.copy()
        args.update(dict(key=key, value=value))
        yield templ('extra', **args)

def showfiles(ctx, templ, **args):
    return showlist(templ, 'file', ctx.files(), **args)

def shownode(ctx, templ, **args):
    return ctx.hex()

def showrev(ctx, templ, **args):
    return ctx.rev()

def showtags(ctx, templ, **args):
    return showlist(templ, 'tag', ctx.tags(), **args)

keywords = {
    'author': showauthor,
    'branches': showbranches,
    'date': showdate,
    'desc': showdescription,
    'extras': showextras,
    'files': showfiles,
    'node': shownode,
    'rev': showrev,
    'tags': showtags,
}

