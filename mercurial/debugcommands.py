# debugcommands.py - command processing for debug* commands
#
# Copyright 2005-2016 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import operator
import os
import random

from .i18n import _
from .node import (
    bin,
    hex,
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
    error,
    exchange,
    extensions,
    fileset,
    hg,
    localrepo,
    lock as lockmod,
    pycompat,
    repair,
    revlog,
    scmutil,
    setdiscovery,
    simplemerge,
    streamclone,
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
