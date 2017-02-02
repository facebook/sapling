# debugcommands.py - command processing for debug* commands
#
# Copyright 2005-2016 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import operator
import os
import random
import socket
import sys
import tempfile
import time

from .i18n import _
from .node import (
    bin,
    hex,
    nullhex,
    nullid,
    short,
)
from . import (
    bundle2,
    changegroup,
    cmdutil,
    commands,
    context,
    dagparser,
    dagutil,
    encoding,
    error,
    exchange,
    extensions,
    fileset,
    hg,
    localrepo,
    lock as lockmod,
    merge as mergemod,
    obsolete,
    policy,
    pvec,
    pycompat,
    repair,
    revlog,
    scmutil,
    setdiscovery,
    simplemerge,
    sslutil,
    streamclone,
    templater,
    treediscovery,
    util,
)

release = lockmod.release

# We reuse the command table from commands because it is easier than
# teaching dispatch about multiple tables.
command = cmdutil.command(commands.table)

@command('debugancestor', [], _('[INDEX] REV1 REV2'), optionalrepo=True)
def debugancestor(ui, repo, *args):
    """find the ancestor revision of two revisions in a given index"""
    if len(args) == 3:
        index, rev1, rev2 = args
        r = revlog.revlog(scmutil.opener(pycompat.getcwd(), audit=False), index)
        lookup = r.lookup
    elif len(args) == 2:
        if not repo:
            raise error.Abort(_('there is no Mercurial repository here '
                                '(.hg not found)'))
        rev1, rev2 = args
        r = repo.changelog
        lookup = repo.lookup
    else:
        raise error.Abort(_('either two or three arguments required'))
    a = r.ancestor(lookup(rev1), lookup(rev2))
    ui.write('%d:%s\n' % (r.rev(a), hex(a)))

@command('debugapplystreamclonebundle', [], 'FILE')
def debugapplystreamclonebundle(ui, repo, fname):
    """apply a stream clone bundle file"""
    f = hg.openpath(ui, fname)
    gen = exchange.readbundle(ui, f, fname)
    gen.apply(repo)

@command('debugbuilddag',
    [('m', 'mergeable-file', None, _('add single file mergeable changes')),
    ('o', 'overwritten-file', None, _('add single file all revs overwrite')),
    ('n', 'new-file', None, _('add new file at each rev'))],
    _('[OPTION]... [TEXT]'))
def debugbuilddag(ui, repo, text=None,
                  mergeable_file=False,
                  overwritten_file=False,
                  new_file=False):
    """builds a repo with a given DAG from scratch in the current empty repo

    The description of the DAG is read from stdin if not given on the
    command line.

    Elements:

     - "+n" is a linear run of n nodes based on the current default parent
     - "." is a single node based on the current default parent
     - "$" resets the default parent to null (implied at the start);
           otherwise the default parent is always the last node created
     - "<p" sets the default parent to the backref p
     - "*p" is a fork at parent p, which is a backref
     - "*p1/p2" is a merge of parents p1 and p2, which are backrefs
     - "/p2" is a merge of the preceding node and p2
     - ":tag" defines a local tag for the preceding node
     - "@branch" sets the named branch for subsequent nodes
     - "#...\\n" is a comment up to the end of the line

    Whitespace between the above elements is ignored.

    A backref is either

     - a number n, which references the node curr-n, where curr is the current
       node, or
     - the name of a local tag you placed earlier using ":tag", or
     - empty to denote the default parent.

    All string valued-elements are either strictly alphanumeric, or must
    be enclosed in double quotes ("..."), with "\\" as escape character.
    """

    if text is None:
        ui.status(_("reading DAG from stdin\n"))
        text = ui.fin.read()

    cl = repo.changelog
    if len(cl) > 0:
        raise error.Abort(_('repository is not empty'))

    # determine number of revs in DAG
    total = 0
    for type, data in dagparser.parsedag(text):
        if type == 'n':
            total += 1

    if mergeable_file:
        linesperrev = 2
        # make a file with k lines per rev
        initialmergedlines = [str(i) for i in xrange(0, total * linesperrev)]
        initialmergedlines.append("")

    tags = []

    wlock = lock = tr = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        tr = repo.transaction("builddag")

        at = -1
        atbranch = 'default'
        nodeids = []
        id = 0
        ui.progress(_('building'), id, unit=_('revisions'), total=total)
        for type, data in dagparser.parsedag(text):
            if type == 'n':
                ui.note(('node %s\n' % str(data)))
                id, ps = data

                files = []
                fctxs = {}

                p2 = None
                if mergeable_file:
                    fn = "mf"
                    p1 = repo[ps[0]]
                    if len(ps) > 1:
                        p2 = repo[ps[1]]
                        pa = p1.ancestor(p2)
                        base, local, other = [x[fn].data() for x in (pa, p1,
                                                                     p2)]
                        m3 = simplemerge.Merge3Text(base, local, other)
                        ml = [l.strip() for l in m3.merge_lines()]
                        ml.append("")
                    elif at > 0:
                        ml = p1[fn].data().split("\n")
                    else:
                        ml = initialmergedlines
                    ml[id * linesperrev] += " r%i" % id
                    mergedtext = "\n".join(ml)
                    files.append(fn)
                    fctxs[fn] = context.memfilectx(repo, fn, mergedtext)

                if overwritten_file:
                    fn = "of"
                    files.append(fn)
                    fctxs[fn] = context.memfilectx(repo, fn, "r%i\n" % id)

                if new_file:
                    fn = "nf%i" % id
                    files.append(fn)
                    fctxs[fn] = context.memfilectx(repo, fn, "r%i\n" % id)
                    if len(ps) > 1:
                        if not p2:
                            p2 = repo[ps[1]]
                        for fn in p2:
                            if fn.startswith("nf"):
                                files.append(fn)
                                fctxs[fn] = p2[fn]

                def fctxfn(repo, cx, path):
                    return fctxs.get(path)

                if len(ps) == 0 or ps[0] < 0:
                    pars = [None, None]
                elif len(ps) == 1:
                    pars = [nodeids[ps[0]], None]
                else:
                    pars = [nodeids[p] for p in ps]
                cx = context.memctx(repo, pars, "r%i" % id, files, fctxfn,
                                    date=(id, 0),
                                    user="debugbuilddag",
                                    extra={'branch': atbranch})
                nodeid = repo.commitctx(cx)
                nodeids.append(nodeid)
                at = id
            elif type == 'l':
                id, name = data
                ui.note(('tag %s\n' % name))
                tags.append("%s %s\n" % (hex(repo.changelog.node(id)), name))
            elif type == 'a':
                ui.note(('branch %s\n' % data))
                atbranch = data
            ui.progress(_('building'), id, unit=_('revisions'), total=total)
        tr.close()

        if tags:
            repo.vfs.write("localtags", "".join(tags))
    finally:
        ui.progress(_('building'), None)
        release(tr, lock, wlock)

def _debugchangegroup(ui, gen, all=None, indent=0, **opts):
    indent_string = ' ' * indent
    if all:
        ui.write(("%sformat: id, p1, p2, cset, delta base, len(delta)\n")
                 % indent_string)

        def showchunks(named):
            ui.write("\n%s%s\n" % (indent_string, named))
            chain = None
            for chunkdata in iter(lambda: gen.deltachunk(chain), {}):
                node = chunkdata['node']
                p1 = chunkdata['p1']
                p2 = chunkdata['p2']
                cs = chunkdata['cs']
                deltabase = chunkdata['deltabase']
                delta = chunkdata['delta']
                ui.write("%s%s %s %s %s %s %s\n" %
                         (indent_string, hex(node), hex(p1), hex(p2),
                          hex(cs), hex(deltabase), len(delta)))
                chain = node

        chunkdata = gen.changelogheader()
        showchunks("changelog")
        chunkdata = gen.manifestheader()
        showchunks("manifest")
        for chunkdata in iter(gen.filelogheader, {}):
            fname = chunkdata['filename']
            showchunks(fname)
    else:
        if isinstance(gen, bundle2.unbundle20):
            raise error.Abort(_('use debugbundle2 for this file'))
        chunkdata = gen.changelogheader()
        chain = None
        for chunkdata in iter(lambda: gen.deltachunk(chain), {}):
            node = chunkdata['node']
            ui.write("%s%s\n" % (indent_string, hex(node)))
            chain = node

def _debugbundle2(ui, gen, all=None, **opts):
    """lists the contents of a bundle2"""
    if not isinstance(gen, bundle2.unbundle20):
        raise error.Abort(_('not a bundle2 file'))
    ui.write(('Stream params: %s\n' % repr(gen.params)))
    for part in gen.iterparts():
        ui.write('%s -- %r\n' % (part.type, repr(part.params)))
        if part.type == 'changegroup':
            version = part.params.get('version', '01')
            cg = changegroup.getunbundler(version, part, 'UN')
            _debugchangegroup(ui, cg, all=all, indent=4, **opts)

@command('debugbundle',
        [('a', 'all', None, _('show all details')),
         ('', 'spec', None, _('print the bundlespec of the bundle'))],
        _('FILE'),
        norepo=True)
def debugbundle(ui, bundlepath, all=None, spec=None, **opts):
    """lists the contents of a bundle"""
    with hg.openpath(ui, bundlepath) as f:
        if spec:
            spec = exchange.getbundlespec(ui, f)
            ui.write('%s\n' % spec)
            return

        gen = exchange.readbundle(ui, f, bundlepath)
        if isinstance(gen, bundle2.unbundle20):
            return _debugbundle2(ui, gen, all=all, **opts)
        _debugchangegroup(ui, gen, all=all, **opts)

@command('debugcheckstate', [], '')
def debugcheckstate(ui, repo):
    """validate the correctness of the current dirstate"""
    parent1, parent2 = repo.dirstate.parents()
    m1 = repo[parent1].manifest()
    m2 = repo[parent2].manifest()
    errors = 0
    for f in repo.dirstate:
        state = repo.dirstate[f]
        if state in "nr" and f not in m1:
            ui.warn(_("%s in state %s, but not in manifest1\n") % (f, state))
            errors += 1
        if state in "a" and f in m1:
            ui.warn(_("%s in state %s, but also in manifest1\n") % (f, state))
            errors += 1
        if state in "m" and f not in m1 and f not in m2:
            ui.warn(_("%s in state %s, but not in either manifest\n") %
                    (f, state))
            errors += 1
    for f in m1:
        state = repo.dirstate[f]
        if state not in "nrm":
            ui.warn(_("%s in manifest1, but listed as state %s") % (f, state))
            errors += 1
    if errors:
        error = _(".hg/dirstate inconsistent with current parent's manifest")
        raise error.Abort(error)

@command('debugcommands', [], _('[COMMAND]'), norepo=True)
def debugcommands(ui, cmd='', *args):
    """list all available commands and options"""
    for cmd, vals in sorted(commands.table.iteritems()):
        cmd = cmd.split('|')[0].strip('^')
        opts = ', '.join([i[1] for i in vals[1]])
        ui.write('%s: %s\n' % (cmd, opts))

@command('debugcomplete',
    [('o', 'options', None, _('show the command options'))],
    _('[-o] CMD'),
    norepo=True)
def debugcomplete(ui, cmd='', **opts):
    """returns the completion list associated with the given command"""

    if opts.get('options'):
        options = []
        otables = [commands.globalopts]
        if cmd:
            aliases, entry = cmdutil.findcmd(cmd, commands.table, False)
            otables.append(entry[1])
        for t in otables:
            for o in t:
                if "(DEPRECATED)" in o[3]:
                    continue
                if o[0]:
                    options.append('-%s' % o[0])
                options.append('--%s' % o[1])
        ui.write("%s\n" % "\n".join(options))
        return

    cmdlist, unused_allcmds = cmdutil.findpossible(cmd, commands.table)
    if ui.verbose:
        cmdlist = [' '.join(c[0]) for c in cmdlist.values()]
    ui.write("%s\n" % "\n".join(sorted(cmdlist)))

@command('debugcreatestreamclonebundle', [], 'FILE')
def debugcreatestreamclonebundle(ui, repo, fname):
    """create a stream clone bundle file

    Stream bundles are special bundles that are essentially archives of
    revlog files. They are commonly used for cloning very quickly.
    """
    requirements, gen = streamclone.generatebundlev1(repo)
    changegroup.writechunks(ui, gen, fname)

    ui.write(_('bundle requirements: %s\n') % ', '.join(sorted(requirements)))

@command('debugdag',
    [('t', 'tags', None, _('use tags as labels')),
    ('b', 'branches', None, _('annotate with branch names')),
    ('', 'dots', None, _('use dots for runs')),
    ('s', 'spaces', None, _('separate elements by spaces'))],
    _('[OPTION]... [FILE [REV]...]'),
    optionalrepo=True)
def debugdag(ui, repo, file_=None, *revs, **opts):
    """format the changelog or an index DAG as a concise textual description

    If you pass a revlog index, the revlog's DAG is emitted. If you list
    revision numbers, they get labeled in the output as rN.

    Otherwise, the changelog DAG of the current repo is emitted.
    """
    spaces = opts.get('spaces')
    dots = opts.get('dots')
    if file_:
        rlog = revlog.revlog(scmutil.opener(pycompat.getcwd(), audit=False),
                             file_)
        revs = set((int(r) for r in revs))
        def events():
            for r in rlog:
                yield 'n', (r, list(p for p in rlog.parentrevs(r)
                                        if p != -1))
                if r in revs:
                    yield 'l', (r, "r%i" % r)
    elif repo:
        cl = repo.changelog
        tags = opts.get('tags')
        branches = opts.get('branches')
        if tags:
            labels = {}
            for l, n in repo.tags().items():
                labels.setdefault(cl.rev(n), []).append(l)
        def events():
            b = "default"
            for r in cl:
                if branches:
                    newb = cl.read(cl.node(r))[5]['branch']
                    if newb != b:
                        yield 'a', newb
                        b = newb
                yield 'n', (r, list(p for p in cl.parentrevs(r)
                                        if p != -1))
                if tags:
                    ls = labels.get(r)
                    if ls:
                        for l in ls:
                            yield 'l', (r, l)
    else:
        raise error.Abort(_('need repo for changelog dag'))

    for line in dagparser.dagtextlines(events(),
                                       addspaces=spaces,
                                       wraplabels=True,
                                       wrapannotations=True,
                                       wrapnonlinear=dots,
                                       usedots=dots,
                                       maxlinewidth=70):
        ui.write(line)
        ui.write("\n")

@command('debugdata', commands.debugrevlogopts, _('-c|-m|FILE REV'))
def debugdata(ui, repo, file_, rev=None, **opts):
    """dump the contents of a data file revision"""
    if opts.get('changelog') or opts.get('manifest') or opts.get('dir'):
        if rev is not None:
            raise error.CommandError('debugdata', _('invalid arguments'))
        file_, rev = None, file_
    elif rev is None:
        raise error.CommandError('debugdata', _('invalid arguments'))
    r = cmdutil.openrevlog(repo, 'debugdata', file_, opts)
    try:
        ui.write(r.revision(r.lookup(rev), raw=True))
    except KeyError:
        raise error.Abort(_('invalid revision identifier %s') % rev)

@command('debugdate',
    [('e', 'extended', None, _('try extended date formats'))],
    _('[-e] DATE [RANGE]'),
    norepo=True, optionalrepo=True)
def debugdate(ui, date, range=None, **opts):
    """parse and display a date"""
    if opts["extended"]:
        d = util.parsedate(date, util.extendeddateformats)
    else:
        d = util.parsedate(date)
    ui.write(("internal: %s %s\n") % d)
    ui.write(("standard: %s\n") % util.datestr(d))
    if range:
        m = util.matchdate(range)
        ui.write(("match: %s\n") % m(d[0]))

@command('debugdeltachain',
    commands.debugrevlogopts + commands.formatteropts,
    _('-c|-m|FILE'),
    optionalrepo=True)
def debugdeltachain(ui, repo, file_=None, **opts):
    """dump information about delta chains in a revlog

    Output can be templatized. Available template keywords are:

    :``rev``:       revision number
    :``chainid``:   delta chain identifier (numbered by unique base)
    :``chainlen``:  delta chain length to this revision
    :``prevrev``:   previous revision in delta chain
    :``deltatype``: role of delta / how it was computed
    :``compsize``:  compressed size of revision
    :``uncompsize``: uncompressed size of revision
    :``chainsize``: total size of compressed revisions in chain
    :``chainratio``: total chain size divided by uncompressed revision size
                    (new delta chains typically start at ratio 2.00)
    :``lindist``:   linear distance from base revision in delta chain to end
                    of this revision
    :``extradist``: total size of revisions not part of this delta chain from
                    base of delta chain to end of this revision; a measurement
                    of how much extra data we need to read/seek across to read
                    the delta chain for this revision
    :``extraratio``: extradist divided by chainsize; another representation of
                    how much unrelated data is needed to load this delta chain
    """
    r = cmdutil.openrevlog(repo, 'debugdeltachain', file_, opts)
    index = r.index
    generaldelta = r.version & revlog.REVLOGGENERALDELTA

    def revinfo(rev):
        e = index[rev]
        compsize = e[1]
        uncompsize = e[2]
        chainsize = 0

        if generaldelta:
            if e[3] == e[5]:
                deltatype = 'p1'
            elif e[3] == e[6]:
                deltatype = 'p2'
            elif e[3] == rev - 1:
                deltatype = 'prev'
            elif e[3] == rev:
                deltatype = 'base'
            else:
                deltatype = 'other'
        else:
            if e[3] == rev:
                deltatype = 'base'
            else:
                deltatype = 'prev'

        chain = r._deltachain(rev)[0]
        for iterrev in chain:
            e = index[iterrev]
            chainsize += e[1]

        return compsize, uncompsize, deltatype, chain, chainsize

    fm = ui.formatter('debugdeltachain', opts)

    fm.plain('    rev  chain# chainlen     prev   delta       '
             'size    rawsize  chainsize     ratio   lindist extradist '
             'extraratio\n')

    chainbases = {}
    for rev in r:
        comp, uncomp, deltatype, chain, chainsize = revinfo(rev)
        chainbase = chain[0]
        chainid = chainbases.setdefault(chainbase, len(chainbases) + 1)
        basestart = r.start(chainbase)
        revstart = r.start(rev)
        lineardist = revstart + comp - basestart
        extradist = lineardist - chainsize
        try:
            prevrev = chain[-2]
        except IndexError:
            prevrev = -1

        chainratio = float(chainsize) / float(uncomp)
        extraratio = float(extradist) / float(chainsize)

        fm.startitem()
        fm.write('rev chainid chainlen prevrev deltatype compsize '
                 'uncompsize chainsize chainratio lindist extradist '
                 'extraratio',
                 '%7d %7d %8d %8d %7s %10d %10d %10d %9.5f %9d %9d %10.5f\n',
                 rev, chainid, len(chain), prevrev, deltatype, comp,
                 uncomp, chainsize, chainratio, lineardist, extradist,
                 extraratio,
                 rev=rev, chainid=chainid, chainlen=len(chain),
                 prevrev=prevrev, deltatype=deltatype, compsize=comp,
                 uncompsize=uncomp, chainsize=chainsize,
                 chainratio=chainratio, lindist=lineardist,
                 extradist=extradist, extraratio=extraratio)

    fm.end()

@command('debugdiscovery',
    [('', 'old', None, _('use old-style discovery')),
    ('', 'nonheads', None,
     _('use old-style discovery with non-heads included')),
    ] + commands.remoteopts,
    _('[-l REV] [-r REV] [-b BRANCH]... [OTHER]'))
def debugdiscovery(ui, repo, remoteurl="default", **opts):
    """runs the changeset discovery protocol in isolation"""
    remoteurl, branches = hg.parseurl(ui.expandpath(remoteurl),
                                      opts.get('branch'))
    remote = hg.peer(repo, opts, remoteurl)
    ui.status(_('comparing with %s\n') % util.hidepassword(remoteurl))

    # make sure tests are repeatable
    random.seed(12323)

    def doit(localheads, remoteheads, remote=remote):
        if opts.get('old'):
            if localheads:
                raise error.Abort('cannot use localheads with old style '
                                 'discovery')
            if not util.safehasattr(remote, 'branches'):
                # enable in-client legacy support
                remote = localrepo.locallegacypeer(remote.local())
            common, _in, hds = treediscovery.findcommonincoming(repo, remote,
                                                                force=True)
            common = set(common)
            if not opts.get('nonheads'):
                ui.write(("unpruned common: %s\n") %
                         " ".join(sorted(short(n) for n in common)))
                dag = dagutil.revlogdag(repo.changelog)
                all = dag.ancestorset(dag.internalizeall(common))
                common = dag.externalizeall(dag.headsetofconnecteds(all))
        else:
            common, any, hds = setdiscovery.findcommonheads(ui, repo, remote)
        common = set(common)
        rheads = set(hds)
        lheads = set(repo.heads())
        ui.write(("common heads: %s\n") %
                 " ".join(sorted(short(n) for n in common)))
        if lheads <= common:
            ui.write(("local is subset\n"))
        elif rheads <= common:
            ui.write(("remote is subset\n"))

    serverlogs = opts.get('serverlog')
    if serverlogs:
        for filename in serverlogs:
            with open(filename, 'r') as logfile:
                line = logfile.readline()
                while line:
                    parts = line.strip().split(';')
                    op = parts[1]
                    if op == 'cg':
                        pass
                    elif op == 'cgss':
                        doit(parts[2].split(' '), parts[3].split(' '))
                    elif op == 'unb':
                        doit(parts[3].split(' '), parts[2].split(' '))
                    line = logfile.readline()
    else:
        remoterevs, _checkout = hg.addbranchrevs(repo, remote, branches,
                                                 opts.get('remote_head'))
        localrevs = opts.get('local_head')
        doit(localrevs, remoterevs)

@command('debugextensions', commands.formatteropts, [], norepo=True)
def debugextensions(ui, **opts):
    '''show information about active extensions'''
    exts = extensions.extensions(ui)
    hgver = util.version()
    fm = ui.formatter('debugextensions', opts)
    for extname, extmod in sorted(exts, key=operator.itemgetter(0)):
        isinternal = extensions.ismoduleinternal(extmod)
        extsource = extmod.__file__
        if isinternal:
            exttestedwith = []  # never expose magic string to users
        else:
            exttestedwith = getattr(extmod, 'testedwith', '').split()
        extbuglink = getattr(extmod, 'buglink', None)

        fm.startitem()

        if ui.quiet or ui.verbose:
            fm.write('name', '%s\n', extname)
        else:
            fm.write('name', '%s', extname)
            if isinternal or hgver in exttestedwith:
                fm.plain('\n')
            elif not exttestedwith:
                fm.plain(_(' (untested!)\n'))
            else:
                lasttestedversion = exttestedwith[-1]
                fm.plain(' (%s!)\n' % lasttestedversion)

        fm.condwrite(ui.verbose and extsource, 'source',
                 _('  location: %s\n'), extsource or "")

        if ui.verbose:
            fm.plain(_('  bundled: %s\n') % ['no', 'yes'][isinternal])
        fm.data(bundled=isinternal)

        fm.condwrite(ui.verbose and exttestedwith, 'testedwith',
                     _('  tested with: %s\n'),
                     fm.formatlist(exttestedwith, name='ver'))

        fm.condwrite(ui.verbose and extbuglink, 'buglink',
                 _('  bug reporting: %s\n'), extbuglink or "")

    fm.end()

@command('debugfileset',
    [('r', 'rev', '', _('apply the filespec on this revision'), _('REV'))],
    _('[-r REV] FILESPEC'))
def debugfileset(ui, repo, expr, **opts):
    '''parse and apply a fileset specification'''
    ctx = scmutil.revsingle(repo, opts.get('rev'), None)
    if ui.verbose:
        tree = fileset.parse(expr)
        ui.note(fileset.prettyformat(tree), "\n")

    for f in ctx.getfileset(expr):
        ui.write("%s\n" % f)

@command('debugfsinfo', [], _('[PATH]'), norepo=True)
def debugfsinfo(ui, path="."):
    """show information detected about current filesystem"""
    util.writefile('.debugfsinfo', '')
    ui.write(('exec: %s\n') % (util.checkexec(path) and 'yes' or 'no'))
    ui.write(('symlink: %s\n') % (util.checklink(path) and 'yes' or 'no'))
    ui.write(('hardlink: %s\n') % (util.checknlink(path) and 'yes' or 'no'))
    ui.write(('case-sensitive: %s\n') % (util.fscasesensitive('.debugfsinfo')
                                and 'yes' or 'no'))
    os.unlink('.debugfsinfo')

@command('debuggetbundle',
    [('H', 'head', [], _('id of head node'), _('ID')),
    ('C', 'common', [], _('id of common node'), _('ID')),
    ('t', 'type', 'bzip2', _('bundle compression type to use'), _('TYPE'))],
    _('REPO FILE [-H|-C ID]...'),
    norepo=True)
def debuggetbundle(ui, repopath, bundlepath, head=None, common=None, **opts):
    """retrieves a bundle from a repo

    Every ID must be a full-length hex node id string. Saves the bundle to the
    given file.
    """
    repo = hg.peer(ui, opts, repopath)
    if not repo.capable('getbundle'):
        raise error.Abort("getbundle() not supported by target repository")
    args = {}
    if common:
        args['common'] = [bin(s) for s in common]
    if head:
        args['heads'] = [bin(s) for s in head]
    # TODO: get desired bundlecaps from command line.
    args['bundlecaps'] = None
    bundle = repo.getbundle('debug', **args)

    bundletype = opts.get('type', 'bzip2').lower()
    btypes = {'none': 'HG10UN',
              'bzip2': 'HG10BZ',
              'gzip': 'HG10GZ',
              'bundle2': 'HG20'}
    bundletype = btypes.get(bundletype)
    if bundletype not in bundle2.bundletypes:
        raise error.Abort(_('unknown bundle type specified with --type'))
    bundle2.writebundle(ui, bundle, bundlepath, bundletype)

@command('debugignore', [], '[FILE]')
def debugignore(ui, repo, *files, **opts):
    """display the combined ignore pattern and information about ignored files

    With no argument display the combined ignore pattern.

    Given space separated file names, shows if the given file is ignored and
    if so, show the ignore rule (file and line number) that matched it.
    """
    ignore = repo.dirstate._ignore
    if not files:
        # Show all the patterns
        includepat = getattr(ignore, 'includepat', None)
        if includepat is not None:
            ui.write("%s\n" % includepat)
        else:
            raise error.Abort(_("no ignore patterns found"))
    else:
        for f in files:
            nf = util.normpath(f)
            ignored = None
            ignoredata = None
            if nf != '.':
                if ignore(nf):
                    ignored = nf
                    ignoredata = repo.dirstate._ignorefileandline(nf)
                else:
                    for p in util.finddirs(nf):
                        if ignore(p):
                            ignored = p
                            ignoredata = repo.dirstate._ignorefileandline(p)
                            break
            if ignored:
                if ignored == nf:
                    ui.write(_("%s is ignored\n") % f)
                else:
                    ui.write(_("%s is ignored because of "
                               "containing folder %s\n")
                             % (f, ignored))
                ignorefile, lineno, line = ignoredata
                ui.write(_("(ignore rule in %s, line %d: '%s')\n")
                         % (ignorefile, lineno, line))
            else:
                ui.write(_("%s is not ignored\n") % f)

@command('debugindex', commands.debugrevlogopts +
    [('f', 'format', 0, _('revlog format'), _('FORMAT'))],
    _('[-f FORMAT] -c|-m|FILE'),
    optionalrepo=True)
def debugindex(ui, repo, file_=None, **opts):
    """dump the contents of an index file"""
    r = cmdutil.openrevlog(repo, 'debugindex', file_, opts)
    format = opts.get('format', 0)
    if format not in (0, 1):
        raise error.Abort(_("unknown format %d") % format)

    generaldelta = r.version & revlog.REVLOGGENERALDELTA
    if generaldelta:
        basehdr = ' delta'
    else:
        basehdr = '  base'

    if ui.debugflag:
        shortfn = hex
    else:
        shortfn = short

    # There might not be anything in r, so have a sane default
    idlen = 12
    for i in r:
        idlen = len(shortfn(r.node(i)))
        break

    if format == 0:
        ui.write(("   rev    offset  length " + basehdr + " linkrev"
                 " %s %s p2\n") % ("nodeid".ljust(idlen), "p1".ljust(idlen)))
    elif format == 1:
        ui.write(("   rev flag   offset   length"
                 "     size " + basehdr + "   link     p1     p2"
                 " %s\n") % "nodeid".rjust(idlen))

    for i in r:
        node = r.node(i)
        if generaldelta:
            base = r.deltaparent(i)
        else:
            base = r.chainbase(i)
        if format == 0:
            try:
                pp = r.parents(node)
            except Exception:
                pp = [nullid, nullid]
            ui.write("% 6d % 9d % 7d % 6d % 7d %s %s %s\n" % (
                    i, r.start(i), r.length(i), base, r.linkrev(i),
                    shortfn(node), shortfn(pp[0]), shortfn(pp[1])))
        elif format == 1:
            pr = r.parentrevs(i)
            ui.write("% 6d %04x % 8d % 8d % 8d % 6d % 6d % 6d % 6d %s\n" % (
                    i, r.flags(i), r.start(i), r.length(i), r.rawsize(i),
                    base, r.linkrev(i), pr[0], pr[1], shortfn(node)))

@command('debugindexdot', commands.debugrevlogopts,
    _('-c|-m|FILE'), optionalrepo=True)
def debugindexdot(ui, repo, file_=None, **opts):
    """dump an index DAG as a graphviz dot file"""
    r = cmdutil.openrevlog(repo, 'debugindexdot', file_, opts)
    ui.write(("digraph G {\n"))
    for i in r:
        node = r.node(i)
        pp = r.parents(node)
        ui.write("\t%d -> %d\n" % (r.rev(pp[0]), i))
        if pp[1] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(pp[1]), i))
    ui.write("}\n")

@command('debuginstall', [] + commands.formatteropts, '', norepo=True)
def debuginstall(ui, **opts):
    '''test Mercurial installation

    Returns 0 on success.
    '''

    def writetemp(contents):
        (fd, name) = tempfile.mkstemp(prefix="hg-debuginstall-")
        f = os.fdopen(fd, "wb")
        f.write(contents)
        f.close()
        return name

    problems = 0

    fm = ui.formatter('debuginstall', opts)
    fm.startitem()

    # encoding
    fm.write('encoding', _("checking encoding (%s)...\n"), encoding.encoding)
    err = None
    try:
        encoding.fromlocal("test")
    except error.Abort as inst:
        err = inst
        problems += 1
    fm.condwrite(err, 'encodingerror', _(" %s\n"
                 " (check that your locale is properly set)\n"), err)

    # Python
    fm.write('pythonexe', _("checking Python executable (%s)\n"),
             pycompat.sysexecutable)
    fm.write('pythonver', _("checking Python version (%s)\n"),
             ("%d.%d.%d" % sys.version_info[:3]))
    fm.write('pythonlib', _("checking Python lib (%s)...\n"),
             os.path.dirname(pycompat.fsencode(os.__file__)))

    security = set(sslutil.supportedprotocols)
    if sslutil.hassni:
        security.add('sni')

    fm.write('pythonsecurity', _("checking Python security support (%s)\n"),
             fm.formatlist(sorted(security), name='protocol',
                           fmt='%s', sep=','))

    # These are warnings, not errors. So don't increment problem count. This
    # may change in the future.
    if 'tls1.2' not in security:
        fm.plain(_('  TLS 1.2 not supported by Python install; '
                   'network connections lack modern security\n'))
    if 'sni' not in security:
        fm.plain(_('  SNI not supported by Python install; may have '
                   'connectivity issues with some servers\n'))

    # TODO print CA cert info

    # hg version
    hgver = util.version()
    fm.write('hgver', _("checking Mercurial version (%s)\n"),
             hgver.split('+')[0])
    fm.write('hgverextra', _("checking Mercurial custom build (%s)\n"),
             '+'.join(hgver.split('+')[1:]))

    # compiled modules
    fm.write('hgmodulepolicy', _("checking module policy (%s)\n"),
             policy.policy)
    fm.write('hgmodules', _("checking installed modules (%s)...\n"),
             os.path.dirname(__file__))

    err = None
    try:
        from . import (
            base85,
            bdiff,
            mpatch,
            osutil,
        )
        dir(bdiff), dir(mpatch), dir(base85), dir(osutil) # quiet pyflakes
    except Exception as inst:
        err = inst
        problems += 1
    fm.condwrite(err, 'extensionserror', " %s\n", err)

    compengines = util.compengines._engines.values()
    fm.write('compengines', _('checking registered compression engines (%s)\n'),
             fm.formatlist(sorted(e.name() for e in compengines),
                           name='compengine', fmt='%s', sep=', '))
    fm.write('compenginesavail', _('checking available compression engines '
                                   '(%s)\n'),
             fm.formatlist(sorted(e.name() for e in compengines
                                  if e.available()),
                           name='compengine', fmt='%s', sep=', '))
    wirecompengines = util.compengines.supportedwireengines(util.SERVERROLE)
    fm.write('compenginesserver', _('checking available compression engines '
                                    'for wire protocol (%s)\n'),
             fm.formatlist([e.name() for e in wirecompengines
                            if e.wireprotosupport()],
                           name='compengine', fmt='%s', sep=', '))

    # templates
    p = templater.templatepaths()
    fm.write('templatedirs', 'checking templates (%s)...\n', ' '.join(p))
    fm.condwrite(not p, '', _(" no template directories found\n"))
    if p:
        m = templater.templatepath("map-cmdline.default")
        if m:
            # template found, check if it is working
            err = None
            try:
                templater.templater.frommapfile(m)
            except Exception as inst:
                err = inst
                p = None
            fm.condwrite(err, 'defaulttemplateerror', " %s\n", err)
        else:
            p = None
        fm.condwrite(p, 'defaulttemplate',
                     _("checking default template (%s)\n"), m)
        fm.condwrite(not m, 'defaulttemplatenotfound',
                     _(" template '%s' not found\n"), "default")
    if not p:
        problems += 1
    fm.condwrite(not p, '',
                 _(" (templates seem to have been installed incorrectly)\n"))

    # editor
    editor = ui.geteditor()
    editor = util.expandpath(editor)
    fm.write('editor', _("checking commit editor... (%s)\n"), editor)
    cmdpath = util.findexe(pycompat.shlexsplit(editor)[0])
    fm.condwrite(not cmdpath and editor == 'vi', 'vinotfound',
                 _(" No commit editor set and can't find %s in PATH\n"
                   " (specify a commit editor in your configuration"
                   " file)\n"), not cmdpath and editor == 'vi' and editor)
    fm.condwrite(not cmdpath and editor != 'vi', 'editornotfound',
                 _(" Can't find editor '%s' in PATH\n"
                   " (specify a commit editor in your configuration"
                   " file)\n"), not cmdpath and editor)
    if not cmdpath and editor != 'vi':
        problems += 1

    # check username
    username = None
    err = None
    try:
        username = ui.username()
    except error.Abort as e:
        err = e
        problems += 1

    fm.condwrite(username, 'username',  _("checking username (%s)\n"), username)
    fm.condwrite(err, 'usernameerror', _("checking username...\n %s\n"
        " (specify a username in your configuration file)\n"), err)

    fm.condwrite(not problems, '',
                 _("no problems detected\n"))
    if not problems:
        fm.data(problems=problems)
    fm.condwrite(problems, 'problems',
                 _("%d problems detected,"
                   " please check your install!\n"), problems)
    fm.end()

    return problems

@command('debugknown', [], _('REPO ID...'), norepo=True)
def debugknown(ui, repopath, *ids, **opts):
    """test whether node ids are known to a repo

    Every ID must be a full-length hex node id string. Returns a list of 0s
    and 1s indicating unknown/known.
    """
    repo = hg.peer(ui, opts, repopath)
    if not repo.capable('known'):
        raise error.Abort("known() not supported by target repository")
    flags = repo.known([bin(s) for s in ids])
    ui.write("%s\n" % ("".join([f and "1" or "0" for f in flags])))

@command('debuglabelcomplete', [], _('LABEL...'))
def debuglabelcomplete(ui, repo, *args):
    '''backwards compatibility with old bash completion scripts (DEPRECATED)'''
    commands.debugnamecomplete(ui, repo, *args)

@command('debuglocks',
         [('L', 'force-lock', None, _('free the store lock (DANGEROUS)')),
          ('W', 'force-wlock', None,
           _('free the working state lock (DANGEROUS)'))],
         _('[OPTION]...'))
def debuglocks(ui, repo, **opts):
    """show or modify state of locks

    By default, this command will show which locks are held. This
    includes the user and process holding the lock, the amount of time
    the lock has been held, and the machine name where the process is
    running if it's not local.

    Locks protect the integrity of Mercurial's data, so should be
    treated with care. System crashes or other interruptions may cause
    locks to not be properly released, though Mercurial will usually
    detect and remove such stale locks automatically.

    However, detecting stale locks may not always be possible (for
    instance, on a shared filesystem). Removing locks may also be
    blocked by filesystem permissions.

    Returns 0 if no locks are held.

    """

    if opts.get('force_lock'):
        repo.svfs.unlink('lock')
    if opts.get('force_wlock'):
        repo.vfs.unlink('wlock')
    if opts.get('force_lock') or opts.get('force_lock'):
        return 0

    now = time.time()
    held = 0

    def report(vfs, name, method):
        # this causes stale locks to get reaped for more accurate reporting
        try:
            l = method(False)
        except error.LockHeld:
            l = None

        if l:
            l.release()
        else:
            try:
                stat = vfs.lstat(name)
                age = now - stat.st_mtime
                user = util.username(stat.st_uid)
                locker = vfs.readlock(name)
                if ":" in locker:
                    host, pid = locker.split(':')
                    if host == socket.gethostname():
                        locker = 'user %s, process %s' % (user, pid)
                    else:
                        locker = 'user %s, process %s, host %s' \
                                 % (user, pid, host)
                ui.write(("%-6s %s (%ds)\n") % (name + ":", locker, age))
                return 1
            except OSError as e:
                if e.errno != errno.ENOENT:
                    raise

        ui.write(("%-6s free\n") % (name + ":"))
        return 0

    held += report(repo.svfs, "lock", repo.lock)
    held += report(repo.vfs, "wlock", repo.wlock)

    return held

@command('debugmergestate', [], '')
def debugmergestate(ui, repo, *args):
    """print merge state

    Use --verbose to print out information about whether v1 or v2 merge state
    was chosen."""
    def _hashornull(h):
        if h == nullhex:
            return 'null'
        else:
            return h

    def printrecords(version):
        ui.write(('* version %s records\n') % version)
        if version == 1:
            records = v1records
        else:
            records = v2records

        for rtype, record in records:
            # pretty print some record types
            if rtype == 'L':
                ui.write(('local: %s\n') % record)
            elif rtype == 'O':
                ui.write(('other: %s\n') % record)
            elif rtype == 'm':
                driver, mdstate = record.split('\0', 1)
                ui.write(('merge driver: %s (state "%s")\n')
                         % (driver, mdstate))
            elif rtype in 'FDC':
                r = record.split('\0')
                f, state, hash, lfile, afile, anode, ofile = r[0:7]
                if version == 1:
                    onode = 'not stored in v1 format'
                    flags = r[7]
                else:
                    onode, flags = r[7:9]
                ui.write(('file: %s (record type "%s", state "%s", hash %s)\n')
                         % (f, rtype, state, _hashornull(hash)))
                ui.write(('  local path: %s (flags "%s")\n') % (lfile, flags))
                ui.write(('  ancestor path: %s (node %s)\n')
                         % (afile, _hashornull(anode)))
                ui.write(('  other path: %s (node %s)\n')
                         % (ofile, _hashornull(onode)))
            elif rtype == 'f':
                filename, rawextras = record.split('\0', 1)
                extras = rawextras.split('\0')
                i = 0
                extrastrings = []
                while i < len(extras):
                    extrastrings.append('%s = %s' % (extras[i], extras[i + 1]))
                    i += 2

                ui.write(('file extras: %s (%s)\n')
                         % (filename, ', '.join(extrastrings)))
            elif rtype == 'l':
                labels = record.split('\0', 2)
                labels = [l for l in labels if len(l) > 0]
                ui.write(('labels:\n'))
                ui.write(('  local: %s\n' % labels[0]))
                ui.write(('  other: %s\n' % labels[1]))
                if len(labels) > 2:
                    ui.write(('  base:  %s\n' % labels[2]))
            else:
                ui.write(('unrecognized entry: %s\t%s\n')
                         % (rtype, record.replace('\0', '\t')))

    # Avoid mergestate.read() since it may raise an exception for unsupported
    # merge state records. We shouldn't be doing this, but this is OK since this
    # command is pretty low-level.
    ms = mergemod.mergestate(repo)

    # sort so that reasonable information is on top
    v1records = ms._readrecordsv1()
    v2records = ms._readrecordsv2()
    order = 'LOml'
    def key(r):
        idx = order.find(r[0])
        if idx == -1:
            return (1, r[1])
        else:
            return (0, idx)
    v1records.sort(key=key)
    v2records.sort(key=key)

    if not v1records and not v2records:
        ui.write(('no merge state found\n'))
    elif not v2records:
        ui.note(('no version 2 merge state\n'))
        printrecords(1)
    elif ms._v1v2match(v1records, v2records):
        ui.note(('v1 and v2 states match: using v2\n'))
        printrecords(2)
    else:
        ui.note(('v1 and v2 states mismatch: using v1\n'))
        printrecords(1)
        if ui.verbose:
            printrecords(2)

@command('debugnamecomplete', [], _('NAME...'))
def debugnamecomplete(ui, repo, *args):
    '''complete "names" - tags, open branch names, bookmark names'''

    names = set()
    # since we previously only listed open branches, we will handle that
    # specially (after this for loop)
    for name, ns in repo.names.iteritems():
        if name != 'branches':
            names.update(ns.listnames(repo))
    names.update(tag for (tag, heads, tip, closed)
                 in repo.branchmap().iterbranches() if not closed)
    completions = set()
    if not args:
        args = ['']
    for a in args:
        completions.update(n for n in names if n.startswith(a))
    ui.write('\n'.join(sorted(completions)))
    ui.write('\n')

@command('debugobsolete',
        [('', 'flags', 0, _('markers flag')),
         ('', 'record-parents', False,
          _('record parent information for the precursor')),
         ('r', 'rev', [], _('display markers relevant to REV')),
         ('', 'index', False, _('display index of the marker')),
         ('', 'delete', [], _('delete markers specified by indices')),
        ] + commands.commitopts2 + commands.formatteropts,
         _('[OBSOLETED [REPLACEMENT ...]]'))
def debugobsolete(ui, repo, precursor=None, *successors, **opts):
    """create arbitrary obsolete marker

    With no arguments, displays the list of obsolescence markers."""

    def parsenodeid(s):
        try:
            # We do not use revsingle/revrange functions here to accept
            # arbitrary node identifiers, possibly not present in the
            # local repository.
            n = bin(s)
            if len(n) != len(nullid):
                raise TypeError()
            return n
        except TypeError:
            raise error.Abort('changeset references must be full hexadecimal '
                             'node identifiers')

    if opts.get('delete'):
        indices = []
        for v in opts.get('delete'):
            try:
                indices.append(int(v))
            except ValueError:
                raise error.Abort(_('invalid index value: %r') % v,
                                  hint=_('use integers for indices'))

        if repo.currenttransaction():
            raise error.Abort(_('cannot delete obsmarkers in the middle '
                                'of transaction.'))

        with repo.lock():
            n = repair.deleteobsmarkers(repo.obsstore, indices)
            ui.write(_('deleted %i obsolescence markers\n') % n)

        return

    if precursor is not None:
        if opts['rev']:
            raise error.Abort('cannot select revision when creating marker')
        metadata = {}
        metadata['user'] = opts['user'] or ui.username()
        succs = tuple(parsenodeid(succ) for succ in successors)
        l = repo.lock()
        try:
            tr = repo.transaction('debugobsolete')
            try:
                date = opts.get('date')
                if date:
                    date = util.parsedate(date)
                else:
                    date = None
                prec = parsenodeid(precursor)
                parents = None
                if opts['record_parents']:
                    if prec not in repo.unfiltered():
                        raise error.Abort('cannot used --record-parents on '
                                         'unknown changesets')
                    parents = repo.unfiltered()[prec].parents()
                    parents = tuple(p.node() for p in parents)
                repo.obsstore.create(tr, prec, succs, opts['flags'],
                                     parents=parents, date=date,
                                     metadata=metadata)
                tr.close()
            except ValueError as exc:
                raise error.Abort(_('bad obsmarker input: %s') % exc)
            finally:
                tr.release()
        finally:
            l.release()
    else:
        if opts['rev']:
            revs = scmutil.revrange(repo, opts['rev'])
            nodes = [repo[r].node() for r in revs]
            markers = list(obsolete.getmarkers(repo, nodes=nodes))
            markers.sort(key=lambda x: x._data)
        else:
            markers = obsolete.getmarkers(repo)

        markerstoiter = markers
        isrelevant = lambda m: True
        if opts.get('rev') and opts.get('index'):
            markerstoiter = obsolete.getmarkers(repo)
            markerset = set(markers)
            isrelevant = lambda m: m in markerset

        fm = ui.formatter('debugobsolete', opts)
        for i, m in enumerate(markerstoiter):
            if not isrelevant(m):
                # marker can be irrelevant when we're iterating over a set
                # of markers (markerstoiter) which is bigger than the set
                # of markers we want to display (markers)
                # this can happen if both --index and --rev options are
                # provided and thus we need to iterate over all of the markers
                # to get the correct indices, but only display the ones that
                # are relevant to --rev value
                continue
            fm.startitem()
            ind = i if opts.get('index') else None
            cmdutil.showmarker(fm, m, index=ind)
        fm.end()

@command('debugpathcomplete',
         [('f', 'full', None, _('complete an entire path')),
          ('n', 'normal', None, _('show only normal files')),
          ('a', 'added', None, _('show only added files')),
          ('r', 'removed', None, _('show only removed files'))],
         _('FILESPEC...'))
def debugpathcomplete(ui, repo, *specs, **opts):
    '''complete part or all of a tracked path

    This command supports shells that offer path name completion. It
    currently completes only files already known to the dirstate.

    Completion extends only to the next path segment unless
    --full is specified, in which case entire paths are used.'''

    def complete(path, acceptable):
        dirstate = repo.dirstate
        spec = os.path.normpath(os.path.join(pycompat.getcwd(), path))
        rootdir = repo.root + pycompat.ossep
        if spec != repo.root and not spec.startswith(rootdir):
            return [], []
        if os.path.isdir(spec):
            spec += '/'
        spec = spec[len(rootdir):]
        fixpaths = pycompat.ossep != '/'
        if fixpaths:
            spec = spec.replace(pycompat.ossep, '/')
        speclen = len(spec)
        fullpaths = opts['full']
        files, dirs = set(), set()
        adddir, addfile = dirs.add, files.add
        for f, st in dirstate.iteritems():
            if f.startswith(spec) and st[0] in acceptable:
                if fixpaths:
                    f = f.replace('/', pycompat.ossep)
                if fullpaths:
                    addfile(f)
                    continue
                s = f.find(pycompat.ossep, speclen)
                if s >= 0:
                    adddir(f[:s])
                else:
                    addfile(f)
        return files, dirs

    acceptable = ''
    if opts['normal']:
        acceptable += 'nm'
    if opts['added']:
        acceptable += 'a'
    if opts['removed']:
        acceptable += 'r'
    cwd = repo.getcwd()
    if not specs:
        specs = ['.']

    files, dirs = set(), set()
    for spec in specs:
        f, d = complete(spec, acceptable or 'nmar')
        files.update(f)
        dirs.update(d)
    files.update(dirs)
    ui.write('\n'.join(repo.pathto(p, cwd) for p in sorted(files)))
    ui.write('\n')

@command('debugpushkey', [], _('REPO NAMESPACE [KEY OLD NEW]'), norepo=True)
def debugpushkey(ui, repopath, namespace, *keyinfo, **opts):
    '''access the pushkey key/value protocol

    With two args, list the keys in the given namespace.

    With five args, set a key to new if it currently is set to old.
    Reports success or failure.
    '''

    target = hg.peer(ui, {}, repopath)
    if keyinfo:
        key, old, new = keyinfo
        r = target.pushkey(namespace, key, old, new)
        ui.status(str(r) + '\n')
        return not r
    else:
        for k, v in sorted(target.listkeys(namespace).iteritems()):
            ui.write("%s\t%s\n" % (k.encode('string-escape'),
                                   v.encode('string-escape')))

@command('debugpvec', [], _('A B'))
def debugpvec(ui, repo, a, b=None):
    ca = scmutil.revsingle(repo, a)
    cb = scmutil.revsingle(repo, b)
    pa = pvec.ctxpvec(ca)
    pb = pvec.ctxpvec(cb)
    if pa == pb:
        rel = "="
    elif pa > pb:
        rel = ">"
    elif pa < pb:
        rel = "<"
    elif pa | pb:
        rel = "|"
    ui.write(_("a: %s\n") % pa)
    ui.write(_("b: %s\n") % pb)
    ui.write(_("depth(a): %d depth(b): %d\n") % (pa._depth, pb._depth))
    ui.write(_("delta: %d hdist: %d distance: %d relation: %s\n") %
             (abs(pa._depth - pb._depth), pvec._hamming(pa._vec, pb._vec),
              pa.distance(pb), rel))

@command('debugrebuilddirstate|debugrebuildstate',
    [('r', 'rev', '', _('revision to rebuild to'), _('REV')),
     ('', 'minimal', None, _('only rebuild files that are inconsistent with '
                             'the working copy parent')),
    ],
    _('[-r REV]'))
def debugrebuilddirstate(ui, repo, rev, **opts):
    """rebuild the dirstate as it would look like for the given revision

    If no revision is specified the first current parent will be used.

    The dirstate will be set to the files of the given revision.
    The actual working directory content or existing dirstate
    information such as adds or removes is not considered.

    ``minimal`` will only rebuild the dirstate status for files that claim to be
    tracked but are not in the parent manifest, or that exist in the parent
    manifest but are not in the dirstate. It will not change adds, removes, or
    modified files that are in the working copy parent.

    One use of this command is to make the next :hg:`status` invocation
    check the actual file content.
    """
    ctx = scmutil.revsingle(repo, rev)
    with repo.wlock():
        dirstate = repo.dirstate
        changedfiles = None
        # See command doc for what minimal does.
        if opts.get('minimal'):
            manifestfiles = set(ctx.manifest().keys())
            dirstatefiles = set(dirstate)
            manifestonly = manifestfiles - dirstatefiles
            dsonly = dirstatefiles - manifestfiles
            dsnotadded = set(f for f in dsonly if dirstate[f] != 'a')
            changedfiles = manifestonly | dsnotadded

        dirstate.rebuild(ctx.node(), ctx.manifest(), changedfiles)

@command('debugupgraderepo', [
    ('o', 'optimize', [], _('extra optimization to perform'), _('NAME')),
    ('', 'run', False, _('performs an upgrade')),
])
def debugupgraderepo(ui, repo, run=False, optimize=None):
    """upgrade a repository to use different features

    If no arguments are specified, the repository is evaluated for upgrade
    and a list of problems and potential optimizations is printed.

    With ``--run``, a repository upgrade is performed. Behavior of the upgrade
    can be influenced via additional arguments. More details will be provided
    by the command output when run without ``--run``.

    During the upgrade, the repository will be locked and no writes will be
    allowed.

    At the end of the upgrade, the repository may not be readable while new
    repository data is swapped in. This window will be as long as it takes to
    rename some directories inside the ``.hg`` directory. On most machines, this
    should complete almost instantaneously and the chances of a consumer being
    unable to access the repository should be low.
    """
    return repair.upgraderepo(ui, repo, run=run, optimize=optimize)
