# commands.py - command processing for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
from node import *
from i18n import gettext as _
demandload(globals(), "os re sys signal shutil imp urllib pdb")
demandload(globals(), "fancyopts ui hg util lock revlog templater bundlerepo")
demandload(globals(), "fnmatch hgweb mdiff random signal tempfile time")
demandload(globals(), "traceback errno socket version struct atexit sets bz2")

class UnknownCommand(Exception):
    """Exception raised if command is not in the command table."""
class AmbiguousCommand(Exception):
    """Exception raised if command shortcut matches more than one command."""

def filterfiles(filters, files):
    l = [x for x in files if x in filters]

    for t in filters:
        if t and t[-1] != "/":
            t += "/"
        l += [x for x in files if x.startswith(t)]
    return l

def relpath(repo, args):
    cwd = repo.getcwd()
    if cwd:
        return [util.normpath(os.path.join(cwd, x)) for x in args]
    return args

def matchpats(repo, pats=[], opts={}, head=''):
    cwd = repo.getcwd()
    if not pats and cwd:
        opts['include'] = [os.path.join(cwd, i) for i in opts['include']]
        opts['exclude'] = [os.path.join(cwd, x) for x in opts['exclude']]
        cwd = ''
    return util.cmdmatcher(repo.root, cwd, pats or ['.'], opts.get('include'),
                           opts.get('exclude'), head)

def makewalk(repo, pats, opts, node=None, head=''):
    files, matchfn, anypats = matchpats(repo, pats, opts, head)
    exact = dict(zip(files, files))
    def walk():
        for src, fn in repo.walk(node=node, files=files, match=matchfn):
            yield src, fn, util.pathto(repo.getcwd(), fn), fn in exact
    return files, matchfn, walk()

def walk(repo, pats, opts, node=None, head=''):
    files, matchfn, results = makewalk(repo, pats, opts, node, head)
    for r in results:
        yield r

def walkchangerevs(ui, repo, pats, opts):
    '''Iterate over files and the revs they changed in.

    Callers most commonly need to iterate backwards over the history
    it is interested in.  Doing so has awful (quadratic-looking)
    performance, so we use iterators in a "windowed" way.

    We walk a window of revisions in the desired order.  Within the
    window, we first walk forwards to gather data, then in the desired
    order (usually backwards) to display it.

    This function returns an (iterator, getchange, matchfn) tuple.  The
    getchange function returns the changelog entry for a numeric
    revision.  The iterator yields 3-tuples.  They will be of one of
    the following forms:

    "window", incrementing, lastrev: stepping through a window,
    positive if walking forwards through revs, last rev in the
    sequence iterated over - use to reset state for the current window

    "add", rev, fns: out-of-order traversal of the given file names
    fns, which changed during revision rev - use to gather data for
    possible display

    "iter", rev, None: in-order traversal of the revs earlier iterated
    over with "add" - use to display data'''

    def increasing_windows(start, end, windowsize=8, sizelimit=512):
        if start < end:
            while start < end:
                yield start, min(windowsize, end-start)
                start += windowsize
                if windowsize < sizelimit:
                    windowsize *= 2
        else:
            while start > end:
                yield start, min(windowsize, start-end-1)
                start -= windowsize
                if windowsize < sizelimit:
                    windowsize *= 2


    files, matchfn, anypats = matchpats(repo, pats, opts)

    if repo.changelog.count() == 0:
        return [], False, matchfn

    revs = map(int, revrange(ui, repo, opts['rev'] or ['tip:0']))
    wanted = {}
    slowpath = anypats
    fncache = {}

    chcache = {}
    def getchange(rev):
        ch = chcache.get(rev)
        if ch is None:
            chcache[rev] = ch = repo.changelog.read(repo.lookup(str(rev)))
        return ch

    if not slowpath and not files:
        # No files, no patterns.  Display all revs.
        wanted = dict(zip(revs, revs))
    if not slowpath:
        # Only files, no patterns.  Check the history of each file.
        def filerevgen(filelog):
            for i, window in increasing_windows(filelog.count()-1, -1):
                revs = []
                for j in xrange(i - window, i + 1):
                    revs.append(filelog.linkrev(filelog.node(j)))
                revs.reverse()
                for rev in revs:
                    yield rev

        minrev, maxrev = min(revs), max(revs)
        for file_ in files:
            filelog = repo.file(file_)
            # A zero count may be a directory or deleted file, so
            # try to find matching entries on the slow path.
            if filelog.count() == 0:
                slowpath = True
                break
            for rev in filerevgen(filelog):
                if rev <= maxrev:
                    if rev < minrev:
                        break
                    fncache.setdefault(rev, [])
                    fncache[rev].append(file_)
                    wanted[rev] = 1
    if slowpath:
        # The slow path checks files modified in every changeset.
        def changerevgen():
            for i, window in increasing_windows(repo.changelog.count()-1, -1):
                for j in xrange(i - window, i + 1):
                    yield j, getchange(j)[3]

        for rev, changefiles in changerevgen():
            matches = filter(matchfn, changefiles)
            if matches:
                fncache[rev] = matches
                wanted[rev] = 1

    def iterate():
        for i, window in increasing_windows(0, len(revs)):
            yield 'window', revs[0] < revs[-1], revs[-1]
            nrevs = [rev for rev in revs[i:i+window]
                     if rev in wanted]
            srevs = list(nrevs)
            srevs.sort()
            for rev in srevs:
                fns = fncache.get(rev) or filter(matchfn, getchange(rev)[3])
                yield 'add', rev, fns
            for rev in nrevs:
                yield 'iter', rev, None
    return iterate(), getchange, matchfn

revrangesep = ':'

def revrange(ui, repo, revs, revlog=None):
    """Yield revision as strings from a list of revision specifications."""
    if revlog is None:
        revlog = repo.changelog
    revcount = revlog.count()
    def fix(val, defval):
        if not val:
            return defval
        try:
            num = int(val)
            if str(num) != val:
                raise ValueError
            if num < 0:
                num += revcount
            if num < 0:
                num = 0
            elif num >= revcount:
                raise ValueError
        except ValueError:
            try:
                num = repo.changelog.rev(repo.lookup(val))
            except KeyError:
                try:
                    num = revlog.rev(revlog.lookup(val))
                except KeyError:
                    raise util.Abort(_('invalid revision identifier %s'), val)
        return num
    seen = {}
    for spec in revs:
        if spec.find(revrangesep) >= 0:
            start, end = spec.split(revrangesep, 1)
            start = fix(start, 0)
            end = fix(end, revcount - 1)
            step = start > end and -1 or 1
            for rev in xrange(start, end+step, step):
                if rev in seen:
                    continue
                seen[rev] = 1
                yield str(rev)
        else:
            rev = fix(spec, None)
            if rev in seen:
                continue
            seen[rev] = 1
            yield str(rev)

def make_filename(repo, r, pat, node=None,
                  total=None, seqno=None, revwidth=None, pathname=None):
    node_expander = {
        'H': lambda: hex(node),
        'R': lambda: str(r.rev(node)),
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
            expander['r'] = lambda: str(r.rev(node)).zfill(revwidth)
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

def make_file(repo, r, pat, node=None,
              total=None, seqno=None, revwidth=None, mode='wb', pathname=None):
    if not pat or pat == '-':
        return 'w' in mode and sys.stdout or sys.stdin
    if hasattr(pat, 'write') and 'w' in mode:
        return pat
    if hasattr(pat, 'read') and 'r' in mode:
        return pat
    return open(make_filename(repo, r, pat, node, total, seqno, revwidth,
                              pathname),
                mode)

def write_bundle(cg, filename, compress=True, fh=None):
    """Write a bundle file, optionally without bz2 compression.

    A file handle (fh) may be passed and is guaranteed to be closed.
    """
    if fh is None:
        fh = open(filename, "wb")

    class nocompress(object):
        def compress(self, x):
            return x
        def flush(self):
            return ""
    try:
        if compress:
            fh.write("HG10")
            z = bz2.BZ2Compressor(9)
        else:
            fh.write("HG11")
            z = nocompress()
        while 1:
            chunk = cg.read(4096)
            if not chunk:
                break
            fh.write(z.compress(chunk))
        fh.write(z.flush())
    except:
        fh.close()
        os.unlink(filename)
        raise
    fh.close()

def dodiff(fp, ui, repo, node1, node2, files=None, match=util.always,
           changes=None, text=False, opts={}):
    if not node1:
        node1 = repo.dirstate.parents()[0]
    # reading the data for node1 early allows it to play nicely
    # with repo.changes and the revlog cache.
    change = repo.changelog.read(node1)
    mmap = repo.manifest.read(change[0])
    date1 = util.datestr(change[2])

    if not changes:
        changes = repo.changes(node1, node2, files, match=match)
    modified, added, removed, deleted, unknown = changes
    if files:
        modified, added, removed = map(lambda x: filterfiles(files, x),
                                       (modified, added, removed))

    if not modified and not added and not removed:
        return

    if node2:
        change = repo.changelog.read(node2)
        mmap2 = repo.manifest.read(change[0])
        date2 = util.datestr(change[2])
        def read(f):
            return repo.file(f).read(mmap2[f])
    else:
        date2 = util.datestr()
        def read(f):
            return repo.wread(f)

    if ui.quiet:
        r = None
    else:
        hexfunc = ui.verbose and hex or short
        r = [hexfunc(node) for node in [node1, node2] if node]

    diffopts = ui.diffopts()
    showfunc = opts.get('show_function') or diffopts['showfunc']
    ignorews = opts.get('ignore_all_space') or diffopts['ignorews']
    for f in modified:
        to = None
        if f in mmap:
            to = repo.file(f).read(mmap[f])
        tn = read(f)
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r, text=text,
                               showfunc=showfunc, ignorews=ignorews))
    for f in added:
        to = None
        tn = read(f)
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r, text=text,
                               showfunc=showfunc, ignorews=ignorews))
    for f in removed:
        to = repo.file(f).read(mmap[f])
        tn = None
        fp.write(mdiff.unidiff(to, date1, tn, date2, f, r, text=text,
                               showfunc=showfunc, ignorews=ignorews))

def trimuser(ui, name, rev, revcache):
    """trim the name of the user who committed a change"""
    user = revcache.get(rev)
    if user is None:
        user = revcache[rev] = ui.shortuser(name)
    return user

class changeset_templater(object):
    '''use templater module to format changeset information.'''

    def __init__(self, ui, repo, mapfile):
        self.t = templater.templater(mapfile, templater.common_filters,
                                     cache={'parent': '{rev}:{node|short} ',
                                            'manifest': '{rev}:{node|short}'})
        self.ui = ui
        self.repo = repo

    def use_template(self, t):
        '''set template string to use'''
        self.t.cache['changeset'] = t

    def write(self, thing):
        '''write expanded template.
        uses in-order recursive traverse of iterators.'''
        for t in thing:
            if hasattr(t, '__iter__'):
                self.write(t)
            else:
                self.ui.write(t)

    def show(self, rev=0, changenode=None, brinfo=None):
        '''show a single changeset or file revision'''
        log = self.repo.changelog
        if changenode is None:
            changenode = log.node(rev)
        elif not rev:
            rev = log.rev(changenode)

        changes = log.read(changenode)

        def showlist(name, values, plural=None, **args):
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
                if noname in self.t:
                    yield self.t(noname, **args)
                return
            if name not in self.t:
                if isinstance(values[0], str):
                    yield ' '.join(values)
                else:
                    for v in values:
                        yield dict(v, **args)
                return
            startname = 'start_' + names
            if startname in self.t:
                yield self.t(startname, **args)
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
                return self.t(tag, **vargs)
            lastname = 'last_' + name
            if lastname in self.t:
                last = values.pop()
            else:
                last = None
            for v in values:
                yield one(v)
            if last is not None:
                yield one(last, tag=lastname)
            endname = 'end_' + names
            if endname in self.t:
                yield self.t(endname, **args)

        if brinfo:
            def showbranches(**args):
                if changenode in brinfo:
                    for x in showlist('branch', brinfo[changenode],
                                      plural='branches', **args):
                        yield x
        else:
            showbranches = ''

        if self.ui.debugflag:
            def showmanifest(**args):
                args = args.copy()
                args.update(dict(rev=self.repo.manifest.rev(changes[0]),
                                 node=hex(changes[0])))
                yield self.t('manifest', **args)
        else:
            showmanifest = ''

        def showparents(**args):
            parents = [[('rev', log.rev(p)), ('node', hex(p))]
                       for p in log.parents(changenode)
                       if self.ui.debugflag or p != nullid]
            if (not self.ui.debugflag and len(parents) == 1 and
                parents[0][0][1] == rev - 1):
                return
            for x in showlist('parent', parents, **args):
                yield x

        def showtags(**args):
            for x in showlist('tag', self.repo.nodetags(changenode), **args):
                yield x

        if self.ui.debugflag:
            files = self.repo.changes(log.parents(changenode)[0], changenode)
            def showfiles(**args):
                for x in showlist('file', files[0], **args): yield x
            def showadds(**args):
                for x in showlist('file_add', files[1], **args): yield x
            def showdels(**args):
                for x in showlist('file_del', files[2], **args): yield x
        else:
            def showfiles(**args):
                for x in showlist('file', changes[3], **args): yield x
            showadds = ''
            showdels = ''

        props = {
            'author': changes[1],
            'branches': showbranches,
            'date': changes[2],
            'desc': changes[4],
            'file_adds': showadds,
            'file_dels': showdels,
            'files': showfiles,
            'manifest': showmanifest,
            'node': hex(changenode),
            'parents': showparents,
            'rev': rev,
            'tags': showtags,
            }

        try:
            if self.ui.debugflag and 'changeset_debug' in self.t:
                key = 'changeset_debug'
            elif self.ui.quiet and 'changeset_quiet' in self.t:
                key = 'changeset_quiet'
            elif self.ui.verbose and 'changeset_verbose' in self.t:
                key = 'changeset_verbose'
            else:
                key = 'changeset'
            self.write(self.t(key, **props))
        except KeyError, inst:
            raise util.Abort(_("%s: no key named '%s'") % (self.t.mapfile,
                                                           inst.args[0]))
        except SyntaxError, inst:
            raise util.Abort(_('%s: %s') % (self.t.mapfile, inst.args[0]))

class changeset_printer(object):
    '''show changeset information when templating not requested.'''

    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo

    def show(self, rev=0, changenode=None, brinfo=None):
        '''show a single changeset or file revision'''
        log = self.repo.changelog
        if changenode is None:
            changenode = log.node(rev)
        elif not rev:
            rev = log.rev(changenode)

        if self.ui.quiet:
            self.ui.write("%d:%s\n" % (rev, short(changenode)))
            return

        changes = log.read(changenode)
        date = util.datestr(changes[2])

        parents = [(log.rev(p), self.ui.verbose and hex(p) or short(p))
                   for p in log.parents(changenode)
                   if self.ui.debugflag or p != nullid]
        if (not self.ui.debugflag and len(parents) == 1 and
            parents[0][0] == rev-1):
            parents = []

        if self.ui.verbose:
            self.ui.write(_("changeset:   %d:%s\n") % (rev, hex(changenode)))
        else:
            self.ui.write(_("changeset:   %d:%s\n") % (rev, short(changenode)))

        for tag in self.repo.nodetags(changenode):
            self.ui.status(_("tag:         %s\n") % tag)
        for parent in parents:
            self.ui.write(_("parent:      %d:%s\n") % parent)

        if brinfo and changenode in brinfo:
            br = brinfo[changenode]
            self.ui.write(_("branch:      %s\n") % " ".join(br))

        self.ui.debug(_("manifest:    %d:%s\n") %
                      (self.repo.manifest.rev(changes[0]), hex(changes[0])))
        self.ui.status(_("user:        %s\n") % changes[1])
        self.ui.status(_("date:        %s\n") % date)

        if self.ui.debugflag:
            files = self.repo.changes(log.parents(changenode)[0], changenode)
            for key, value in zip([_("files:"), _("files+:"), _("files-:")],
                                  files):
                if value:
                    self.ui.note("%-12s %s\n" % (key, " ".join(value)))
        else:
            self.ui.note(_("files:       %s\n") % " ".join(changes[3]))

        description = changes[4].strip()
        if description:
            if self.ui.verbose:
                self.ui.status(_("description:\n"))
                self.ui.status(description)
                self.ui.status("\n\n")
            else:
                self.ui.status(_("summary:     %s\n") %
                               description.splitlines()[0])
        self.ui.status("\n")

def show_changeset(ui, repo, opts):
    '''show one changeset.  uses template or regular display.  caller
    can pass in 'style' and 'template' options in opts.'''

    tmpl = opts.get('template')
    if tmpl:
        tmpl = templater.parsestring(tmpl, quoted=False)
    else:
        tmpl = ui.config('ui', 'logtemplate')
        if tmpl: tmpl = templater.parsestring(tmpl)
    mapfile = opts.get('style') or ui.config('ui', 'style')
    if tmpl or mapfile:
        if mapfile:
            if not os.path.isfile(mapfile):
                mapname = templater.templatepath('map-cmdline.' + mapfile)
                if not mapname: mapname = templater.templatepath(mapfile)
                if mapname: mapfile = mapname
        try:
            t = changeset_templater(ui, repo, mapfile)
        except SyntaxError, inst:
            raise util.Abort(inst.args[0])
        if tmpl: t.use_template(tmpl)
        return t
    return changeset_printer(ui, repo)

def show_version(ui):
    """output version and copyright information"""
    ui.write(_("Mercurial Distributed SCM (version %s)\n")
             % version.get_version())
    ui.status(_(
        "\nCopyright (C) 2005 Matt Mackall <mpm@selenic.com>\n"
        "This is free software; see the source for copying conditions. "
        "There is NO\nwarranty; "
        "not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.\n"
    ))

def help_(ui, cmd=None, with_version=False):
    """show help for a given command or all commands"""
    option_lists = []
    if cmd and cmd != 'shortlist':
        if with_version:
            show_version(ui)
            ui.write('\n')
        aliases, i = find(cmd)
        # synopsis
        ui.write("%s\n\n" % i[2])

        # description
        doc = i[0].__doc__
        if not doc:
            doc = _("(No help text available)")
        if ui.quiet:
            doc = doc.splitlines(0)[0]
        ui.write("%s\n" % doc.rstrip())

        if not ui.quiet:
            # aliases
            if len(aliases) > 1:
                ui.write(_("\naliases: %s\n") % ', '.join(aliases[1:]))

            # options
            if i[1]:
                option_lists.append(("options", i[1]))

    else:
        # program name
        if ui.verbose or with_version:
            show_version(ui)
        else:
            ui.status(_("Mercurial Distributed SCM\n"))
        ui.status('\n')

        # list of commands
        if cmd == "shortlist":
            ui.status(_('basic commands (use "hg help" '
                        'for the full list or option "-v" for details):\n\n'))
        elif ui.verbose:
            ui.status(_('list of commands:\n\n'))
        else:
            ui.status(_('list of commands (use "hg help -v" '
                        'to show aliases and global options):\n\n'))

        h = {}
        cmds = {}
        for c, e in table.items():
            f = c.split("|")[0]
            if cmd == "shortlist" and not f.startswith("^"):
                continue
            f = f.lstrip("^")
            if not ui.debugflag and f.startswith("debug"):
                continue
            doc = e[0].__doc__
            if not doc:
                doc = _("(No help text available)")
            h[f] = doc.splitlines(0)[0].rstrip()
            cmds[f] = c.lstrip("^")

        fns = h.keys()
        fns.sort()
        m = max(map(len, fns))
        for f in fns:
            if ui.verbose:
                commands = cmds[f].replace("|",", ")
                ui.write(" %s:\n      %s\n"%(commands, h[f]))
            else:
                ui.write(' %-*s   %s\n' % (m, f, h[f]))

    # global options
    if ui.verbose:
        option_lists.append(("global options", globalopts))

    # list all option lists
    opt_output = []
    for title, options in option_lists:
        opt_output.append(("\n%s:\n" % title, None))
        for shortopt, longopt, default, desc in options:
            opt_output.append(("%2s%s" % (shortopt and "-%s" % shortopt,
                                          longopt and " --%s" % longopt),
                               "%s%s" % (desc,
                                         default
                                         and _(" (default: %s)") % default
                                         or "")))

    if opt_output:
        opts_len = max([len(line[0]) for line in opt_output if line[1]])
        for first, second in opt_output:
            if second:
                ui.write(" %-*s  %s\n" % (opts_len, first, second))
            else:
                ui.write("%s\n" % first)

# Commands start here, listed alphabetically

def add(ui, repo, *pats, **opts):
    """add the specified files on the next commit

    Schedule files to be version controlled and added to the repository.

    The files will be added to the repository at the next commit.

    If no names are given, add all files in the repository.
    """

    names = []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if exact:
            if ui.verbose:
                ui.status(_('adding %s\n') % rel)
            names.append(abs)
        elif repo.dirstate.state(abs) == '?':
            ui.status(_('adding %s\n') % rel)
            names.append(abs)
    repo.add(names)

def addremove(ui, repo, *pats, **opts):
    """add all new files, delete all missing files

    Add all new files and remove all missing files from the repository.

    New files are ignored if they match any of the patterns in .hgignore. As
    with add, these changes take effect at the next commit.
    """
    return addremove_lock(ui, repo, pats, opts)

def addremove_lock(ui, repo, pats, opts, wlock=None):
    add, remove = [], []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if src == 'f' and repo.dirstate.state(abs) == '?':
            add.append(abs)
            if ui.verbose or not exact:
                ui.status(_('adding %s\n') % ((pats and rel) or abs))
        if repo.dirstate.state(abs) != 'r' and not os.path.exists(rel):
            remove.append(abs)
            if ui.verbose or not exact:
                ui.status(_('removing %s\n') % ((pats and rel) or abs))
    repo.add(add, wlock=wlock)
    repo.remove(remove, wlock=wlock)

def annotate(ui, repo, *pats, **opts):
    """show changeset information per file line

    List changes in files, showing the revision id responsible for each line

    This command is useful to discover who did a change or when a change took
    place.

    Without the -a option, annotate will avoid processing files it
    detects as binary. With -a, annotate will generate an annotation
    anyway, probably with undesirable results.
    """
    def getnode(rev):
        return short(repo.changelog.node(rev))

    ucache = {}
    def getname(rev):
        cl = repo.changelog.read(repo.changelog.node(rev))
        return trimuser(ui, cl[1], rev, ucache)

    dcache = {}
    def getdate(rev):
        datestr = dcache.get(rev)
        if datestr is None:
            cl = repo.changelog.read(repo.changelog.node(rev))
            datestr = dcache[rev] = util.datestr(cl[2])
        return datestr

    if not pats:
        raise util.Abort(_('at least one file name or pattern required'))

    opmap = [['user', getname], ['number', str], ['changeset', getnode],
             ['date', getdate]]
    if not opts['user'] and not opts['changeset'] and not opts['date']:
        opts['number'] = 1

    if opts['rev']:
        node = repo.changelog.lookup(opts['rev'])
    else:
        node = repo.dirstate.parents()[0]
    change = repo.changelog.read(node)
    mmap = repo.manifest.read(change[0])

    for src, abs, rel, exact in walk(repo, pats, opts, node=node):
        f = repo.file(abs)
        if not opts['text'] and util.binary(f.read(mmap[abs])):
            ui.write(_("%s: binary file\n") % ((pats and rel) or abs))
            continue

        lines = f.annotate(mmap[abs])
        pieces = []

        for o, f in opmap:
            if opts[o]:
                l = [f(n) for n, dummy in lines]
                if l:
                    m = max(map(len, l))
                    pieces.append(["%*s" % (m, x) for x in l])

        if pieces:
            for p, l in zip(zip(*pieces), lines):
                ui.write("%s: %s" % (" ".join(p), l[1]))

def bundle(ui, repo, fname, dest="default-push", **opts):
    """create a changegroup file

    Generate a compressed changegroup file collecting all changesets
    not found in the other repository.

    This file can then be transferred using conventional means and
    applied to another repository with the unbundle command. This is
    useful when native push and pull are not available or when
    exporting an entire repository is undesirable. The standard file
    extension is ".hg".

    Unlike import/export, this exactly preserves all changeset
    contents including permissions, rename data, and revision history.
    """
    dest = ui.expandpath(dest)
    other = hg.repository(ui, dest)
    o = repo.findoutgoing(other, force=opts['force'])
    cg = repo.changegroup(o, 'bundle')
    write_bundle(cg, fname)

def cat(ui, repo, file1, *pats, **opts):
    """output the latest or given revisions of files

    Print the specified files as they were at the given revision.
    If no revision is given then the tip is used.

    Output may be to a file, in which case the name of the file is
    given using a format string.  The formatting rules are the same as
    for the export command, with the following additions:

    %s   basename of file being printed
    %d   dirname of file being printed, or '.' if in repo root
    %p   root-relative path name of file being printed
    """
    mf = {}
    rev = opts['rev']
    if rev:
        node = repo.lookup(rev)
    else:
        node = repo.changelog.tip()
    change = repo.changelog.read(node)
    mf = repo.manifest.read(change[0])
    for src, abs, rel, exact in walk(repo, (file1,) + pats, opts, node):
        r = repo.file(abs)
        n = mf[abs]
        fp = make_file(repo, r, opts['output'], node=n, pathname=abs)
        fp.write(r.read(n))

def clone(ui, source, dest=None, **opts):
    """make a copy of an existing repository

    Create a copy of an existing repository in a new directory.

    If no destination directory name is specified, it defaults to the
    basename of the source.

    The location of the source is added to the new repository's
    .hg/hgrc file, as the default to be used for future pulls.

    For efficiency, hardlinks are used for cloning whenever the source
    and destination are on the same filesystem.  Some filesystems,
    such as AFS, implement hardlinking incorrectly, but do not report
    errors.  In these cases, use the --pull option to avoid
    hardlinking.

    See pull for valid source format details.
    """
    if dest is None:
        dest = os.path.basename(os.path.normpath(source))

    if os.path.exists(dest):
        raise util.Abort(_("destination '%s' already exists"), dest)

    dest = os.path.realpath(dest)

    class Dircleanup(object):
        def __init__(self, dir_):
            self.rmtree = shutil.rmtree
            self.dir_ = dir_
            os.mkdir(dir_)
        def close(self):
            self.dir_ = None
        def __del__(self):
            if self.dir_:
                self.rmtree(self.dir_, True)

    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    source = ui.expandpath(source)

    d = Dircleanup(dest)
    abspath = source
    other = hg.repository(ui, source)

    copy = False
    if other.dev() != -1:
        abspath = os.path.abspath(source)
        if not opts['pull'] and not opts['rev']:
            copy = True

    if copy:
        try:
            # we use a lock here because if we race with commit, we
            # can end up with extra data in the cloned revlogs that's
            # not pointed to by changesets, thus causing verify to
            # fail
            l1 = other.lock()
        except lock.LockException:
            copy = False

    if copy:
        # we lock here to avoid premature writing to the target
        os.mkdir(os.path.join(dest, ".hg"))
        l2 = lock.lock(os.path.join(dest, ".hg", "lock"))

        files = "data 00manifest.d 00manifest.i 00changelog.d 00changelog.i"
        for f in files.split():
            src = os.path.join(source, ".hg", f)
            dst = os.path.join(dest, ".hg", f)
            try:
                util.copyfiles(src, dst)
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise

        repo = hg.repository(ui, dest)

    else:
        revs = None
        if opts['rev']:
            if not other.local():
                error = _("clone -r not supported yet for remote repositories.")
                raise util.Abort(error)
            else:
                revs = [other.lookup(rev) for rev in opts['rev']]
        repo = hg.repository(ui, dest, create=1)
        repo.pull(other, heads = revs)

    f = repo.opener("hgrc", "w", text=True)
    f.write("[paths]\n")
    f.write("default = %s\n" % abspath)
    f.close()

    if not opts['noupdate']:
        update(repo.ui, repo)

    d.close()

def commit(ui, repo, *pats, **opts):
    """commit the specified files or all outstanding changes

    Commit changes to the given files into the repository.

    If a list of files is omitted, all changes reported by "hg status"
    will be commited.

    The HGEDITOR or EDITOR environment variables are used to start an
    editor to add a commit comment.
    """
    message = opts['message']
    logfile = opts['logfile']

    if message and logfile:
        raise util.Abort(_('options --message and --logfile are mutually '
                           'exclusive'))
    if not message and logfile:
        try:
            if logfile == '-':
                message = sys.stdin.read()
            else:
                message = open(logfile).read()
        except IOError, inst:
            raise util.Abort(_("can't read commit message '%s': %s") %
                             (logfile, inst.strerror))

    if opts['addremove']:
        addremove(ui, repo, *pats, **opts)
    fns, match, anypats = matchpats(repo, pats, opts)
    if pats:
        modified, added, removed, deleted, unknown = (
            repo.changes(files=fns, match=match))
        files = modified + added + removed
    else:
        files = []
    try:
        repo.commit(files, message, opts['user'], opts['date'], match)
    except ValueError, inst:
        raise util.Abort(str(inst))

def docopy(ui, repo, pats, opts, wlock):
    # called with the repo lock held
    cwd = repo.getcwd()
    errors = 0
    copied = []
    targets = {}

    def okaytocopy(abs, rel, exact):
        reasons = {'?': _('is not managed'),
                   'a': _('has been marked for add'),
                   'r': _('has been marked for remove')}
        state = repo.dirstate.state(abs)
        reason = reasons.get(state)
        if reason:
            if state == 'a':
                origsrc = repo.dirstate.copied(abs)
                if origsrc is not None:
                    return origsrc
            if exact:
                ui.warn(_('%s: not copying - file %s\n') % (rel, reason))
        else:
            return abs

    def copy(origsrc, abssrc, relsrc, target, exact):
        abstarget = util.canonpath(repo.root, cwd, target)
        reltarget = util.pathto(cwd, abstarget)
        prevsrc = targets.get(abstarget)
        if prevsrc is not None:
            ui.warn(_('%s: not overwriting - %s collides with %s\n') %
                    (reltarget, abssrc, prevsrc))
            return
        if (not opts['after'] and os.path.exists(reltarget) or
            opts['after'] and repo.dirstate.state(abstarget) not in '?r'):
            if not opts['force']:
                ui.warn(_('%s: not overwriting - file exists\n') %
                        reltarget)
                return
            if not opts['after']:
                os.unlink(reltarget)
        if opts['after']:
            if not os.path.exists(reltarget):
                return
        else:
            targetdir = os.path.dirname(reltarget) or '.'
            if not os.path.isdir(targetdir):
                os.makedirs(targetdir)
            try:
                restore = repo.dirstate.state(abstarget) == 'r'
                if restore:
                    repo.undelete([abstarget], wlock)
                try:
                    shutil.copyfile(relsrc, reltarget)
                    shutil.copymode(relsrc, reltarget)
                    restore = False
                finally:
                    if restore:
                        repo.remove([abstarget], wlock)
            except shutil.Error, inst:
                raise util.Abort(str(inst))
            except IOError, inst:
                if inst.errno == errno.ENOENT:
                    ui.warn(_('%s: deleted in working copy\n') % relsrc)
                else:
                    ui.warn(_('%s: cannot copy - %s\n') %
                            (relsrc, inst.strerror))
                    errors += 1
                    return
        if ui.verbose or not exact:
            ui.status(_('copying %s to %s\n') % (relsrc, reltarget))
        targets[abstarget] = abssrc
        if abstarget != origsrc:
            repo.copy(origsrc, abstarget, wlock)
        copied.append((abssrc, relsrc, exact))

    def targetpathfn(pat, dest, srcs):
        if os.path.isdir(pat):
            abspfx = util.canonpath(repo.root, cwd, pat)
            if destdirexists:
                striplen = len(os.path.split(abspfx)[0])
            else:
                striplen = len(abspfx)
            if striplen:
                striplen += len(os.sep)
            res = lambda p: os.path.join(dest, p[striplen:])
        elif destdirexists:
            res = lambda p: os.path.join(dest, os.path.basename(p))
        else:
            res = lambda p: dest
        return res

    def targetpathafterfn(pat, dest, srcs):
        if util.patkind(pat, None)[0]:
            # a mercurial pattern
            res = lambda p: os.path.join(dest, os.path.basename(p))
        else:
            abspfx = util.canonpath(repo.root, cwd, pat)
            if len(abspfx) < len(srcs[0][0]):
                # A directory. Either the target path contains the last
                # component of the source path or it does not.
                def evalpath(striplen):
                    score = 0
                    for s in srcs:
                        t = os.path.join(dest, s[0][striplen:])
                        if os.path.exists(t):
                            score += 1
                    return score

                striplen = len(abspfx)
                if striplen:
                    striplen += len(os.sep)
                if os.path.isdir(os.path.join(dest, os.path.split(abspfx)[1])):
                    score = evalpath(striplen)
                    striplen1 = len(os.path.split(abspfx)[0])
                    if striplen1:
                        striplen1 += len(os.sep)
                    if evalpath(striplen1) > score:
                        striplen = striplen1
                res = lambda p: os.path.join(dest, p[striplen:])
            else:
                # a file
                if destdirexists:
                    res = lambda p: os.path.join(dest, os.path.basename(p))
                else:
                    res = lambda p: dest
        return res


    pats = list(pats)
    if not pats:
        raise util.Abort(_('no source or destination specified'))
    if len(pats) == 1:
        raise util.Abort(_('no destination specified'))
    dest = pats.pop()
    destdirexists = os.path.isdir(dest)
    if (len(pats) > 1 or util.patkind(pats[0], None)[0]) and not destdirexists:
        raise util.Abort(_('with multiple sources, destination must be an '
                         'existing directory'))
    if opts['after']:
        tfn = targetpathafterfn
    else:
        tfn = targetpathfn
    copylist = []
    for pat in pats:
        srcs = []
        for tag, abssrc, relsrc, exact in walk(repo, [pat], opts):
            origsrc = okaytocopy(abssrc, relsrc, exact)
            if origsrc:
                srcs.append((origsrc, abssrc, relsrc, exact))
        if not srcs:
            continue
        copylist.append((tfn(pat, dest, srcs), srcs))
    if not copylist:
        raise util.Abort(_('no files to copy'))

    for targetpath, srcs in copylist:
        for origsrc, abssrc, relsrc, exact in srcs:
            copy(origsrc, abssrc, relsrc, targetpath(abssrc), exact)

    if errors:
        ui.warn(_('(consider using --after)\n'))
    return errors, copied

def copy(ui, repo, *pats, **opts):
    """mark files as copied for the next commit

    Mark dest as having copies of source files.  If dest is a
    directory, copies are put in that directory.  If dest is a file,
    there can only be one source.

    By default, this command copies the contents of files as they
    stand in the working directory.  If invoked with --after, the
    operation is recorded, but no copying is performed.

    This command takes effect in the next commit.

    NOTE: This command should be treated as experimental. While it
    should properly record copied files, this information is not yet
    fully used by merge, nor fully reported by log.
    """
    try:
        wlock = repo.wlock(0)
        errs, copied = docopy(ui, repo, pats, opts, wlock)
    except lock.LockHeld, inst:
        ui.warn(_("repository lock held by %s\n") % inst.args[0])
        errs = 1
    return errs

def debugancestor(ui, index, rev1, rev2):
    """find the ancestor revision of two revisions in a given index"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), index, "")
    a = r.ancestor(r.lookup(rev1), r.lookup(rev2))
    ui.write("%d:%s\n" % (r.rev(a), hex(a)))

def debugcomplete(ui, cmd):
    """returns the completion list associated with the given command"""
    clist = findpossible(cmd).keys()
    clist.sort()
    ui.write("%s\n" % " ".join(clist))

def debugrebuildstate(ui, repo, rev=None):
    """rebuild the dirstate as it would look like for the given revision"""
    if not rev:
        rev = repo.changelog.tip()
    else:
        rev = repo.lookup(rev)
    change = repo.changelog.read(rev)
    n = change[0]
    files = repo.manifest.readflags(n)
    wlock = repo.wlock()
    repo.dirstate.rebuild(rev, files.iteritems())

def debugcheckstate(ui, repo):
    """validate the correctness of the current dirstate"""
    parent1, parent2 = repo.dirstate.parents()
    repo.dirstate.read()
    dc = repo.dirstate.map
    keys = dc.keys()
    keys.sort()
    m1n = repo.changelog.read(parent1)[0]
    m2n = repo.changelog.read(parent2)[0]
    m1 = repo.manifest.read(m1n)
    m2 = repo.manifest.read(m2n)
    errors = 0
    for f in dc:
        state = repo.dirstate.state(f)
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
        state = repo.dirstate.state(f)
        if state not in "nrm":
            ui.warn(_("%s in manifest1, but listed as state %s") % (f, state))
            errors += 1
    if errors:
        error = _(".hg/dirstate inconsistent with current parent's manifest")
        raise util.Abort(error)

def debugconfig(ui, repo):
    """show combined config settings from all hgrc files"""
    for section, name, value in ui.walkconfig():
        ui.write('%s.%s=%s\n' % (section, name, value))

def debugsetparents(ui, repo, rev1, rev2=None):
    """manually set the parents of the current working directory

    This is useful for writing repository conversion tools, but should
    be used with care.
    """

    if not rev2:
        rev2 = hex(nullid)

    repo.dirstate.setparents(repo.lookup(rev1), repo.lookup(rev2))

def debugstate(ui, repo):
    """show the contents of the current dirstate"""
    repo.dirstate.read()
    dc = repo.dirstate.map
    keys = dc.keys()
    keys.sort()
    for file_ in keys:
        ui.write("%c %3o %10d %s %s\n"
                 % (dc[file_][0], dc[file_][1] & 0777, dc[file_][2],
                    time.strftime("%x %X",
                                  time.localtime(dc[file_][3])), file_))
    for f in repo.dirstate.copies:
        ui.write(_("copy: %s -> %s\n") % (repo.dirstate.copies[f], f))

def debugdata(ui, file_, rev):
    """dump the contents of an data file revision"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False),
                      file_[:-2] + ".i", file_)
    try:
        ui.write(r.revision(r.lookup(rev)))
    except KeyError:
        raise util.Abort(_('invalid revision identifier %s'), rev)

def debugindex(ui, file_):
    """dump the contents of an index file"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), file_, "")
    ui.write("   rev    offset  length   base linkrev" +
             " nodeid       p1           p2\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("% 6d % 9d % 7d % 6d % 7d %s %s %s\n" % (
                i, e[0], e[1], e[2], e[3],
            short(e[6]), short(e[4]), short(e[5])))

def debugindexdot(ui, file_):
    """dump an index DAG as a .dot file"""
    r = revlog.revlog(util.opener(os.getcwd(), audit=False), file_, "")
    ui.write("digraph G {\n")
    for i in range(r.count()):
        e = r.index[i]
        ui.write("\t%d -> %d\n" % (r.rev(e[4]), i))
        if e[5] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(e[5]), i))
    ui.write("}\n")

def debugrename(ui, repo, file, rev=None):
    """dump rename information"""
    r = repo.file(relpath(repo, [file])[0])
    if rev:
        try:
            # assume all revision numbers are for changesets
            n = repo.lookup(rev)
            change = repo.changelog.read(n)
            m = repo.manifest.read(change[0])
            n = m[relpath(repo, [file])[0]]
        except (hg.RepoError, KeyError):
            n = r.lookup(rev)
    else:
        n = r.tip()
    m = r.renamed(n)
    if m:
        ui.write(_("renamed from %s:%s\n") % (m[0], hex(m[1])))
    else:
        ui.write(_("not renamed\n"))

def debugwalk(ui, repo, *pats, **opts):
    """show how files match on given patterns"""
    items = list(walk(repo, pats, opts))
    if not items:
        return
    fmt = '%%s  %%-%ds  %%-%ds  %%s' % (
        max([len(abs) for (src, abs, rel, exact) in items]),
        max([len(rel) for (src, abs, rel, exact) in items]))
    for src, abs, rel, exact in items:
        line = fmt % (src, abs, rel, exact and 'exact' or '')
        ui.write("%s\n" % line.rstrip())

def diff(ui, repo, *pats, **opts):
    """diff repository (or selected files)

    Show differences between revisions for the specified files.

    Differences between files are shown using the unified diff format.

    When two revision arguments are given, then changes are shown
    between those revisions. If only one revision is specified then
    that revision is compared to the working directory, and, when no
    revisions are specified, the working directory files are compared
    to its parent.

    Without the -a option, diff will avoid generating diffs of files
    it detects as binary. With -a, diff will generate a diff anyway,
    probably with undesirable results.
    """
    node1, node2 = None, None
    revs = [repo.lookup(x) for x in opts['rev']]

    if len(revs) > 0:
        node1 = revs[0]
    if len(revs) > 1:
        node2 = revs[1]
    if len(revs) > 2:
        raise util.Abort(_("too many revisions to diff"))

    fns, matchfn, anypats = matchpats(repo, pats, opts)

    dodiff(sys.stdout, ui, repo, node1, node2, fns, match=matchfn,
           text=opts['text'], opts=opts)

def doexport(ui, repo, changeset, seqno, total, revwidth, opts):
    node = repo.lookup(changeset)
    parents = [p for p in repo.changelog.parents(node) if p != nullid]
    if opts['switch_parent']:
        parents.reverse()
    prev = (parents and parents[0]) or nullid
    change = repo.changelog.read(node)

    fp = make_file(repo, repo.changelog, opts['output'],
                   node=node, total=total, seqno=seqno,
                   revwidth=revwidth)
    if fp != sys.stdout:
        ui.note("%s\n" % fp.name)

    fp.write("# HG changeset patch\n")
    fp.write("# User %s\n" % change[1])
    fp.write("# Node ID %s\n" % hex(node))
    fp.write("# Parent  %s\n" % hex(prev))
    if len(parents) > 1:
        fp.write("# Parent  %s\n" % hex(parents[1]))
    fp.write(change[4].rstrip())
    fp.write("\n\n")

    dodiff(fp, ui, repo, prev, node, text=opts['text'])
    if fp != sys.stdout:
        fp.close()

def export(ui, repo, *changesets, **opts):
    """dump the header and diffs for one or more changesets

    Print the changeset header and diffs for one or more revisions.

    The information shown in the changeset header is: author,
    changeset hash, parent and commit comment.

    Output may be to a file, in which case the name of the file is
    given using a format string.  The formatting rules are as follows:

    %%   literal "%" character
    %H   changeset hash (40 bytes of hexadecimal)
    %N   number of patches being generated
    %R   changeset revision number
    %b   basename of the exporting repository
    %h   short-form changeset hash (12 bytes of hexadecimal)
    %n   zero-padded sequence number, starting at 1
    %r   zero-padded changeset revision number

    Without the -a option, export will avoid generating diffs of files
    it detects as binary. With -a, export will generate a diff anyway,
    probably with undesirable results.

    With the --switch-parent option, the diff will be against the second
    parent. It can be useful to review a merge.
    """
    if not changesets:
        raise util.Abort(_("export requires at least one changeset"))
    seqno = 0
    revs = list(revrange(ui, repo, changesets))
    total = len(revs)
    revwidth = max(map(len, revs))
    msg = len(revs) > 1 and _("Exporting patches:\n") or _("Exporting patch:\n")
    ui.note(msg)
    for cset in revs:
        seqno += 1
        doexport(ui, repo, cset, seqno, total, revwidth, opts)

def forget(ui, repo, *pats, **opts):
    """don't add the specified files on the next commit

    Undo an 'hg add' scheduled for the next commit.
    """
    forget = []
    for src, abs, rel, exact in walk(repo, pats, opts):
        if repo.dirstate.state(abs) == 'a':
            forget.append(abs)
            if ui.verbose or not exact:
                ui.status(_('forgetting %s\n') % ((pats and rel) or abs))
    repo.forget(forget)

def grep(ui, repo, pattern, *pats, **opts):
    """search for a pattern in specified files and revisions

    Search revisions of files for a regular expression.

    This command behaves differently than Unix grep.  It only accepts
    Python/Perl regexps.  It searches repository history, not the
    working directory.  It always prints the revision number in which
    a match appears.

    By default, grep only prints output for the first revision of a
    file in which it finds a match.  To get it to print every revision
    that contains a change in match status ("-" for a match that
    becomes a non-match, or "+" for a non-match that becomes a match),
    use the --all flag.
    """
    reflags = 0
    if opts['ignore_case']:
        reflags |= re.I
    regexp = re.compile(pattern, reflags)
    sep, eol = ':', '\n'
    if opts['print0']:
        sep = eol = '\0'

    fcache = {}
    def getfile(fn):
        if fn not in fcache:
            fcache[fn] = repo.file(fn)
        return fcache[fn]

    def matchlines(body):
        begin = 0
        linenum = 0
        while True:
            match = regexp.search(body, begin)
            if not match:
                break
            mstart, mend = match.span()
            linenum += body.count('\n', begin, mstart) + 1
            lstart = body.rfind('\n', begin, mstart) + 1 or begin
            lend = body.find('\n', mend)
            yield linenum, mstart - lstart, mend - lstart, body[lstart:lend]
            begin = lend + 1

    class linestate(object):
        def __init__(self, line, linenum, colstart, colend):
            self.line = line
            self.linenum = linenum
            self.colstart = colstart
            self.colend = colend
        def __eq__(self, other):
            return self.line == other.line
        def __hash__(self):
            return hash(self.line)

    matches = {}
    def grepbody(fn, rev, body):
        matches[rev].setdefault(fn, {})
        m = matches[rev][fn]
        for lnum, cstart, cend, line in matchlines(body):
            s = linestate(line, lnum, cstart, cend)
            m[s] = s

    # FIXME: prev isn't used, why ?
    prev = {}
    ucache = {}
    def display(fn, rev, states, prevstates):
        diff = list(sets.Set(states).symmetric_difference(sets.Set(prevstates)))
        diff.sort(lambda x, y: cmp(x.linenum, y.linenum))
        counts = {'-': 0, '+': 0}
        filerevmatches = {}
        for l in diff:
            if incrementing or not opts['all']:
                change = ((l in prevstates) and '-') or '+'
                r = rev
            else:
                change = ((l in states) and '-') or '+'
                r = prev[fn]
            cols = [fn, str(rev)]
            if opts['line_number']:
                cols.append(str(l.linenum))
            if opts['all']:
                cols.append(change)
            if opts['user']:
                cols.append(trimuser(ui, getchange(rev)[1], rev,
                                                  ucache))
            if opts['files_with_matches']:
                c = (fn, rev)
                if c in filerevmatches:
                    continue
                filerevmatches[c] = 1
            else:
                cols.append(l.line)
            ui.write(sep.join(cols), eol)
            counts[change] += 1
        return counts['+'], counts['-']

    fstate = {}
    skip = {}
    changeiter, getchange, matchfn = walkchangerevs(ui, repo, pats, opts)
    count = 0
    incrementing = False
    for st, rev, fns in changeiter:
        if st == 'window':
            incrementing = rev
            matches.clear()
        elif st == 'add':
            change = repo.changelog.read(repo.lookup(str(rev)))
            mf = repo.manifest.read(change[0])
            matches[rev] = {}
            for fn in fns:
                if fn in skip:
                    continue
                fstate.setdefault(fn, {})
                try:
                    grepbody(fn, rev, getfile(fn).read(mf[fn]))
                except KeyError:
                    pass
        elif st == 'iter':
            states = matches[rev].items()
            states.sort()
            for fn, m in states:
                if fn in skip:
                    continue
                if incrementing or not opts['all'] or fstate[fn]:
                    pos, neg = display(fn, rev, m, fstate[fn])
                    count += pos + neg
                    if pos and not opts['all']:
                        skip[fn] = True
                fstate[fn] = m
                prev[fn] = rev

    if not incrementing:
        fstate = fstate.items()
        fstate.sort()
        for fn, state in fstate:
            if fn in skip:
                continue
            display(fn, rev, {}, state)
    return (count == 0 and 1) or 0

def heads(ui, repo, **opts):
    """show current repository heads

    Show all repository head changesets.

    Repository "heads" are changesets that don't have children
    changesets. They are where development generally takes place and
    are the usual targets for update and merge operations.
    """
    if opts['rev']:
        heads = repo.heads(repo.lookup(opts['rev']))
    else:
        heads = repo.heads()
    br = None
    if opts['branches']:
        br = repo.branchlookup(heads)
    displayer = show_changeset(ui, repo, opts)
    for n in heads:
        displayer.show(changenode=n, brinfo=br)

def identify(ui, repo):
    """print information about the working copy

    Print a short summary of the current state of the repo.

    This summary identifies the repository state using one or two parent
    hash identifiers, followed by a "+" if there are uncommitted changes
    in the working directory, followed by a list of tags for this revision.
    """
    parents = [p for p in repo.dirstate.parents() if p != nullid]
    if not parents:
        ui.write(_("unknown\n"))
        return

    hexfunc = ui.verbose and hex or short
    modified, added, removed, deleted, unknown = repo.changes()
    output = ["%s%s" %
              ('+'.join([hexfunc(parent) for parent in parents]),
              (modified or added or removed or deleted) and "+" or "")]

    if not ui.quiet:
        # multiple tags for a single parent separated by '/'
        parenttags = ['/'.join(tags)
                      for tags in map(repo.nodetags, parents) if tags]
        # tags for multiple parents separated by ' + '
        if parenttags:
            output.append(' + '.join(parenttags))

    ui.write("%s\n" % ' '.join(output))

def import_(ui, repo, patch1, *patches, **opts):
    """import an ordered set of patches

    Import a list of patches and commit them individually.

    If there are outstanding changes in the working directory, import
    will abort unless given the -f flag.

    If a patch looks like a mail message (its first line starts with
    "From " or looks like an RFC822 header), it will not be applied
    unless the -f option is used.  The importer neither parses nor
    discards mail headers, so use -f only to override the "mailness"
    safety check, not to import a real mail message.
    """
    patches = (patch1,) + patches

    if not opts['force']:
        modified, added, removed, deleted, unknown = repo.changes()
        if modified or added or removed or deleted:
            raise util.Abort(_("outstanding uncommitted changes"))

    d = opts["base"]
    strip = opts["strip"]

    mailre = re.compile(r'(?:From |[\w-]+:)')

    # attempt to detect the start of a patch
    # (this heuristic is borrowed from quilt)
    diffre = re.compile(r'(?:Index:[ \t]|diff[ \t]|RCS file: |' +
                        'retrieving revision [0-9]+(\.[0-9]+)*$|' +
                        '(---|\*\*\*)[ \t])')

    for patch in patches:
        ui.status(_("applying %s\n") % patch)
        pf = os.path.join(d, patch)

        message = []
        user = None
        hgpatch = False
        for line in file(pf):
            line = line.rstrip()
            if (not message and not hgpatch and
                   mailre.match(line) and not opts['force']):
                if len(line) > 35:
                    line = line[:32] + '...'
                raise util.Abort(_('first line looks like a '
                                   'mail header: ') + line)
            if diffre.match(line):
                break
            elif hgpatch:
                # parse values when importing the result of an hg export
                if line.startswith("# User "):
                    user = line[7:]
                    ui.debug(_('User: %s\n') % user)
                elif not line.startswith("# ") and line:
                    message.append(line)
                    hgpatch = False
            elif line == '# HG changeset patch':
                hgpatch = True
                message = []       # We may have collected garbage
            else:
                message.append(line)

        # make sure message isn't empty
        if not message:
            message = _("imported patch %s\n") % patch
        else:
            message = "%s\n" % '\n'.join(message)
        ui.debug(_('message:\n%s\n') % message)

        files = util.patch(strip, pf, ui)

        if len(files) > 0:
            addremove(ui, repo, *files)
        repo.commit(files, message, user)

def incoming(ui, repo, source="default", **opts):
    """show new changesets found in source

    Show new changesets found in the specified repo or the default
    pull repo. These are the changesets that would be pulled if a pull
    was requested.

    For remote repository, using --bundle avoids downloading the changesets
    twice if the incoming is followed by a pull.
    """
    source = ui.expandpath(source)
    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    other = hg.repository(ui, source)
    incoming = repo.findincoming(other, force=opts["force"])
    if not incoming:
        return

    cleanup = None
    fname = opts["bundle"]
    if fname:
        # create a bundle (uncompressed if other repo is not local)
        f = open(fname, "wb")
    elif not other.local():
        # create an uncompressed temporary bundle
        fd, fname = tempfile.mkstemp(suffix=".hg", prefix="hg-incoming-")
        f = os.fdopen(fd, "wb")
        cleanup = fname

    if fname:
        cg = other.changegroup(incoming, "incoming")
        write_bundle(cg, fname, compress=other.local(), fh=f)
        # write_bundle closed f for us.
        if not other.local():
            # use the created uncompressed bundlerepo
            other = bundlerepo.bundlerepository(ui, repo.root, fname)

    o = other.changelog.nodesbetween(incoming)[0]
    if opts['newest_first']:
        o.reverse()
    displayer = show_changeset(ui, other, opts)
    for n in o:
        parents = [p for p in other.changelog.parents(n) if p != nullid]
        if opts['no_merges'] and len(parents) == 2:
            continue
        displayer.show(changenode=n)
        if opts['patch']:
            prev = (parents and parents[0]) or nullid
            dodiff(ui, ui, other, prev, n)
            ui.write("\n")

    if cleanup:
        other.close()                   # explicit close for unlink
        os.unlink(cleanup)

def init(ui, dest="."):
    """create a new repository in the given directory

    Initialize a new repository in the given directory.  If the given
    directory does not exist, it is created.

    If no directory is given, the current directory is used.
    """
    if not os.path.exists(dest):
        os.mkdir(dest)
    hg.repository(ui, dest, create=1)

def locate(ui, repo, *pats, **opts):
    """locate files matching specific patterns

    Print all files under Mercurial control whose names match the
    given patterns.

    This command searches the current directory and its
    subdirectories.  To search an entire repository, move to the root
    of the repository.

    If no patterns are given to match, this command prints all file
    names.

    If you want to feed the output of this command into the "xargs"
    command, use the "-0" option to both this command and "xargs".
    This will avoid the problem of "xargs" treating single filenames
    that contain white space as multiple filenames.
    """
    end = opts['print0'] and '\0' or '\n'
    rev = opts['rev']
    if rev:
        node = repo.lookup(rev)
    else:
        node = None

    for src, abs, rel, exact in walk(repo, pats, opts, node=node,
                                     head='(?:.*/|)'):
        if not node and repo.dirstate.state(abs) == '?':
            continue
        if opts['fullpath']:
            ui.write(os.path.join(repo.root, abs), end)
        else:
            ui.write(((pats and rel) or abs), end)

def log(ui, repo, *pats, **opts):
    """show revision history of entire repository or files

    Print the revision history of the specified files or the entire project.

    By default this command outputs: changeset id and hash, tags,
    non-trivial parents, user, date and time, and a summary for each
    commit. When the -v/--verbose switch is used, the list of changed
    files and full commit message is shown.
    """
    class dui(object):
        # Implement and delegate some ui protocol.  Save hunks of
        # output for later display in the desired order.
        def __init__(self, ui):
            self.ui = ui
            self.hunk = {}
        def bump(self, rev):
            self.rev = rev
            self.hunk[rev] = []
        def note(self, *args):
            if self.verbose:
                self.write(*args)
        def status(self, *args):
            if not self.quiet:
                self.write(*args)
        def write(self, *args):
            self.hunk[self.rev].append(args)
        def debug(self, *args):
            if self.debugflag:
                self.write(*args)
        def __getattr__(self, key):
            return getattr(self.ui, key)

    changeiter, getchange, matchfn = walkchangerevs(ui, repo, pats, opts)

    if opts['limit']:
        try:
            limit = int(opts['limit'])
        except ValueError:
            raise util.Abort(_('limit must be a positive integer'))
        if limit <= 0: raise util.Abort(_('limit must be positive'))
    else:
        limit = sys.maxint
    count = 0

    displayer = show_changeset(ui, repo, opts)
    for st, rev, fns in changeiter:
        if st == 'window':
            du = dui(ui)
            displayer.ui = du
        elif st == 'add':
            du.bump(rev)
            changenode = repo.changelog.node(rev)
            parents = [p for p in repo.changelog.parents(changenode)
                       if p != nullid]
            if opts['no_merges'] and len(parents) == 2:
                continue
            if opts['only_merges'] and len(parents) != 2:
                continue

            if opts['keyword']:
                changes = getchange(rev)
                miss = 0
                for k in [kw.lower() for kw in opts['keyword']]:
                    if not (k in changes[1].lower() or
                            k in changes[4].lower() or
                            k in " ".join(changes[3][:20]).lower()):
                        miss = 1
                        break
                if miss:
                    continue

            br = None
            if opts['branches']:
                br = repo.branchlookup([repo.changelog.node(rev)])

            displayer.show(rev, brinfo=br)
            if opts['patch']:
                prev = (parents and parents[0]) or nullid
                dodiff(du, du, repo, prev, changenode, match=matchfn)
                du.write("\n\n")
        elif st == 'iter':
            if count == limit: break
            if du.hunk[rev]:
                count += 1
                for args in du.hunk[rev]:
                    ui.write(*args)

def manifest(ui, repo, rev=None):
    """output the latest or given revision of the project manifest

    Print a list of version controlled files for the given revision.

    The manifest is the list of files being version controlled. If no revision
    is given then the tip is used.
    """
    if rev:
        try:
            # assume all revision numbers are for changesets
            n = repo.lookup(rev)
            change = repo.changelog.read(n)
            n = change[0]
        except hg.RepoError:
            n = repo.manifest.lookup(rev)
    else:
        n = repo.manifest.tip()
    m = repo.manifest.read(n)
    mf = repo.manifest.readflags(n)
    files = m.keys()
    files.sort()

    for f in files:
        ui.write("%40s %3s %s\n" % (hex(m[f]), mf[f] and "755" or "644", f))

def outgoing(ui, repo, dest="default-push", **opts):
    """show changesets not found in destination

    Show changesets not found in the specified destination repo or the
    default push repo. These are the changesets that would be pushed
    if a push was requested.

    See pull for valid source format details.
    """
    dest = ui.expandpath(dest)
    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    other = hg.repository(ui, dest)
    o = repo.findoutgoing(other, force=opts['force'])
    o = repo.changelog.nodesbetween(o)[0]
    if opts['newest_first']:
        o.reverse()
    displayer = show_changeset(ui, repo, opts)
    for n in o:
        parents = [p for p in repo.changelog.parents(n) if p != nullid]
        if opts['no_merges'] and len(parents) == 2:
            continue
        displayer.show(changenode=n)
        if opts['patch']:
            prev = (parents and parents[0]) or nullid
            dodiff(ui, ui, repo, prev, n)
            ui.write("\n")

def parents(ui, repo, rev=None, branches=None, **opts):
    """show the parents of the working dir or revision

    Print the working directory's parent revisions.
    """
    if rev:
        p = repo.changelog.parents(repo.lookup(rev))
    else:
        p = repo.dirstate.parents()

    br = None
    if branches is not None:
        br = repo.branchlookup(p)
    displayer = show_changeset(ui, repo, opts)
    for n in p:
        if n != nullid:
            displayer.show(changenode=n, brinfo=br)

def paths(ui, repo, search=None):
    """show definition of symbolic path names

    Show definition of symbolic path name NAME. If no name is given, show
    definition of available names.

    Path names are defined in the [paths] section of /etc/mercurial/hgrc
    and $HOME/.hgrc.  If run inside a repository, .hg/hgrc is used, too.
    """
    if search:
        for name, path in ui.configitems("paths"):
            if name == search:
                ui.write("%s\n" % path)
                return
        ui.warn(_("not found!\n"))
        return 1
    else:
        for name, path in ui.configitems("paths"):
            ui.write("%s = %s\n" % (name, path))

def pull(ui, repo, source="default", **opts):
    """pull changes from the specified source

    Pull changes from a remote repository to a local one.

    This finds all changes from the repository at the specified path
    or URL and adds them to the local repository. By default, this
    does not update the copy of the project in the working directory.

    Valid URLs are of the form:

      local/filesystem/path
      http://[user@]host[:port][/path]
      https://[user@]host[:port][/path]
      ssh://[user@]host[:port][/path]

    SSH requires an accessible shell account on the destination machine
    and a copy of hg in the remote path.  With SSH, paths are relative
    to the remote user's home directory by default; use two slashes at
    the start of a path to specify it as relative to the filesystem root.
    """
    source = ui.expandpath(source)
    ui.status(_('pulling from %s\n') % (source))

    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    other = hg.repository(ui, source)
    revs = None
    if opts['rev'] and not other.local():
        raise util.Abort(_("pull -r doesn't work for remote repositories yet"))
    elif opts['rev']:
        revs = [other.lookup(rev) for rev in opts['rev']]
    r = repo.pull(other, heads=revs, force=opts['force'])
    if not r:
        if opts['update']:
            return update(ui, repo)
        else:
            ui.status(_("(run 'hg update' to get a working copy)\n"))

    return r

def push(ui, repo, dest="default-push", **opts):
    """push changes to the specified destination

    Push changes from the local repository to the given destination.

    This is the symmetrical operation for pull. It helps to move
    changes from the current repository to a different one. If the
    destination is local this is identical to a pull in that directory
    from the current one.

    By default, push will refuse to run if it detects the result would
    increase the number of remote heads. This generally indicates the
    the client has forgotten to sync and merge before pushing.

    Valid URLs are of the form:

      local/filesystem/path
      ssh://[user@]host[:port][/path]

    SSH requires an accessible shell account on the destination
    machine and a copy of hg in the remote path.
    """
    dest = ui.expandpath(dest)
    ui.status('pushing to %s\n' % (dest))

    if opts['ssh']:
        ui.setconfig("ui", "ssh", opts['ssh'])
    if opts['remotecmd']:
        ui.setconfig("ui", "remotecmd", opts['remotecmd'])

    other = hg.repository(ui, dest)
    revs = None
    if opts['rev']:
        revs = [repo.lookup(rev) for rev in opts['rev']]
    r = repo.push(other, opts['force'], revs=revs)
    return r

def rawcommit(ui, repo, *flist, **rc):
    """raw commit interface (DEPRECATED)

    (DEPRECATED)
    Lowlevel commit, for use in helper scripts.

    This command is not intended to be used by normal users, as it is
    primarily useful for importing from other SCMs.

    This command is now deprecated and will be removed in a future
    release, please use debugsetparents and commit instead.
    """

    ui.warn(_("(the rawcommit command is deprecated)\n"))

    message = rc['message']
    if not message and rc['logfile']:
        try:
            message = open(rc['logfile']).read()
        except IOError:
            pass
    if not message and not rc['logfile']:
        raise util.Abort(_("missing commit message"))

    files = relpath(repo, list(flist))
    if rc['files']:
        files += open(rc['files']).read().splitlines()

    rc['parent'] = map(repo.lookup, rc['parent'])

    try:
        repo.rawcommit(files, message, rc['user'], rc['date'], *rc['parent'])
    except ValueError, inst:
        raise util.Abort(str(inst))

def recover(ui, repo):
    """roll back an interrupted transaction

    Recover from an interrupted commit or pull.

    This command tries to fix the repository status after an interrupted
    operation. It should only be necessary when Mercurial suggests it.
    """
    if repo.recover():
        return repo.verify()
    return False

def remove(ui, repo, pat, *pats, **opts):
    """remove the specified files on the next commit

    Schedule the indicated files for removal from the repository.

    This command schedules the files to be removed at the next commit.
    This only removes files from the current branch, not from the
    entire project history.  If the files still exist in the working
    directory, they will be deleted from it.
    """
    names = []
    def okaytoremove(abs, rel, exact):
        modified, added, removed, deleted, unknown = repo.changes(files=[abs])
        reason = None
        if modified and not opts['force']:
            reason = _('is modified')
        elif added:
            reason = _('has been marked for add')
        elif unknown:
            reason = _('is not managed')
        if reason:
            if exact:
                ui.warn(_('not removing %s: file %s\n') % (rel, reason))
        else:
            return True
    for src, abs, rel, exact in walk(repo, (pat,) + pats, opts):
        if okaytoremove(abs, rel, exact):
            if ui.verbose or not exact:
                ui.status(_('removing %s\n') % rel)
            names.append(abs)
    repo.remove(names, unlink=True)

def rename(ui, repo, *pats, **opts):
    """rename files; equivalent of copy + remove

    Mark dest as copies of sources; mark sources for deletion.  If
    dest is a directory, copies are put in that directory.  If dest is
    a file, there can only be one source.

    By default, this command copies the contents of files as they
    stand in the working directory.  If invoked with --after, the
    operation is recorded, but no copying is performed.

    This command takes effect in the next commit.

    NOTE: This command should be treated as experimental. While it
    should properly record rename files, this information is not yet
    fully used by merge, nor fully reported by log.
    """
    try:
        wlock = repo.wlock(0)
        errs, copied = docopy(ui, repo, pats, opts, wlock)
        names = []
        for abs, rel, exact in copied:
            if ui.verbose or not exact:
                ui.status(_('removing %s\n') % rel)
            names.append(abs)
        repo.remove(names, True, wlock)
    except lock.LockHeld, inst:
        ui.warn(_("repository lock held by %s\n") % inst.args[0])
        errs = 1
    return errs

def revert(ui, repo, *pats, **opts):
    """revert modified files or dirs back to their unmodified states

    In its default mode, it reverts any uncommitted modifications made
    to the named files or directories.  This restores the contents of
    the affected files to an unmodified state.

    Using the -r option, it reverts the given files or directories to
    their state as of an earlier revision.  This can be helpful to "roll
    back" some or all of a change that should not have been committed.

    Revert modifies the working directory.  It does not commit any
    changes, or change the parent of the current working directory.

    If a file has been deleted, it is recreated.  If the executable
    mode of a file was changed, it is reset.

    If names are given, all files matching the names are reverted.

    If no arguments are given, all files in the repository are reverted.
    """
    node = opts['rev'] and repo.lookup(opts['rev']) or \
           repo.dirstate.parents()[0]

    files, choose, anypats = matchpats(repo, pats, opts)
    modified, added, removed, deleted, unknown = repo.changes(match=choose)
    repo.forget(added)
    repo.undelete(removed)

    return repo.update(node, False, True, choose, False)

def root(ui, repo):
    """print the root (top) of the current working dir

    Print the root directory of the current repository.
    """
    ui.write(repo.root + "\n")

def serve(ui, repo, **opts):
    """export the repository via HTTP

    Start a local HTTP repository browser and pull server.

    By default, the server logs accesses to stdout and errors to
    stderr.  Use the "-A" and "-E" options to log to files.
    """

    if opts["stdio"]:
        fin, fout = sys.stdin, sys.stdout
        sys.stdout = sys.stderr

        # Prevent insertion/deletion of CRs
        util.set_binary(fin)
        util.set_binary(fout)

        def getarg():
            argline = fin.readline()[:-1]
            arg, l = argline.split()
            val = fin.read(int(l))
            return arg, val
        def respond(v):
            fout.write("%d\n" % len(v))
            fout.write(v)
            fout.flush()

        lock = None

        while 1:
            cmd = fin.readline()[:-1]
            if cmd == '':
                return
            if cmd == "heads":
                h = repo.heads()
                respond(" ".join(map(hex, h)) + "\n")
            if cmd == "lock":
                lock = repo.lock()
                respond("")
            if cmd == "unlock":
                if lock:
                    lock.release()
                lock = None
                respond("")
            elif cmd == "branches":
                arg, nodes = getarg()
                nodes = map(bin, nodes.split(" "))
                r = []
                for b in repo.branches(nodes):
                    r.append(" ".join(map(hex, b)) + "\n")
                respond("".join(r))
            elif cmd == "between":
                arg, pairs = getarg()
                pairs = [map(bin, p.split("-")) for p in pairs.split(" ")]
                r = []
                for b in repo.between(pairs):
                    r.append(" ".join(map(hex, b)) + "\n")
                respond("".join(r))
            elif cmd == "changegroup":
                nodes = []
                arg, roots = getarg()
                nodes = map(bin, roots.split(" "))

                cg = repo.changegroup(nodes, 'serve')
                while 1:
                    d = cg.read(4096)
                    if not d:
                        break
                    fout.write(d)

                fout.flush()

            elif cmd == "addchangegroup":
                if not lock:
                    respond("not locked")
                    continue
                respond("")

                r = repo.addchangegroup(fin)
                respond("")

    optlist = "name templates style address port ipv6 accesslog errorlog"
    for o in optlist.split():
        if opts[o]:
            ui.setconfig("web", o, opts[o])

    if opts['daemon'] and not opts['daemon_pipefds']:
        rfd, wfd = os.pipe()
        args = sys.argv[:]
        args.append('--daemon-pipefds=%d,%d' % (rfd, wfd))
        pid = os.spawnvp(os.P_NOWAIT | getattr(os, 'P_DETACH', 0),
                         args[0], args)
        os.close(wfd)
        os.read(rfd, 1)
        os._exit(0)

    try:
        httpd = hgweb.create_server(repo)
    except socket.error, inst:
        raise util.Abort(_('cannot start server: ') + inst.args[1])

    if ui.verbose:
        addr, port = httpd.socket.getsockname()
        if addr == '0.0.0.0':
            addr = socket.gethostname()
        else:
            try:
                addr = socket.gethostbyaddr(addr)[0]
            except socket.error:
                pass
        if port != 80:
            ui.status(_('listening at http://%s:%d/\n') % (addr, port))
        else:
            ui.status(_('listening at http://%s/\n') % addr)

    if opts['pid_file']:
        fp = open(opts['pid_file'], 'w')
        fp.write(str(os.getpid()))
        fp.close()

    if opts['daemon_pipefds']:
        rfd, wfd = [int(x) for x in opts['daemon_pipefds'].split(',')]
        os.close(rfd)
        os.write(wfd, 'y')
        os.close(wfd)
        sys.stdout.flush()
        sys.stderr.flush()
        fd = os.open(util.nulldev, os.O_RDWR)
        if fd != 0: os.dup2(fd, 0)
        if fd != 1: os.dup2(fd, 1)
        if fd != 2: os.dup2(fd, 2)
        if fd not in (0, 1, 2): os.close(fd)

    httpd.serve_forever()

def status(ui, repo, *pats, **opts):
    """show changed files in the working directory

    Show changed files in the repository.  If names are
    given, only files that match are shown.

    The codes used to show the status of files are:
    M = modified
    A = added
    R = removed
    ! = deleted, but still tracked
    ? = not tracked
    """

    files, matchfn, anypats = matchpats(repo, pats, opts)
    cwd = (pats and repo.getcwd()) or ''
    modified, added, removed, deleted, unknown = [
        [util.pathto(cwd, x) for x in n]
        for n in repo.changes(files=files, match=matchfn)]

    changetypes = [('modified', 'M', modified),
                   ('added', 'A', added),
                   ('removed', 'R', removed),
                   ('deleted', '!', deleted),
                   ('unknown', '?', unknown)]

    end = opts['print0'] and '\0' or '\n'

    for opt, char, changes in ([ct for ct in changetypes if opts[ct[0]]]
                               or changetypes):
        if opts['no_status']:
            format = "%%s%s" % end
        else:
            format = "%s %%s%s" % (char, end)

        for f in changes:
            ui.write(format % f)

def tag(ui, repo, name, rev_=None, **opts):
    """add a tag for the current tip or a given revision

    Name a particular revision using <name>.

    Tags are used to name particular revisions of the repository and are
    very useful to compare different revision, to go back to significant
    earlier versions or to mark branch points as releases, etc.

    If no revision is given, the tip is used.

    To facilitate version control, distribution, and merging of tags,
    they are stored as a file named ".hgtags" which is managed
    similarly to other project files and can be hand-edited if
    necessary.  The file '.hg/localtags' is used for local tags (not
    shared among repositories).
    """
    if name == "tip":
        raise util.Abort(_("the name 'tip' is reserved"))
    if rev_ is not None:
        ui.warn(_("use of 'hg tag NAME [REV]' is deprecated, "
                  "please use 'hg tag [-r REV] NAME' instead\n"))
        if opts['rev']:
            raise util.Abort(_("use only one form to specify the revision"))
    if opts['rev']:
        rev_ = opts['rev']
    if rev_:
        r = hex(repo.lookup(rev_))
    else:
        r = hex(repo.changelog.tip())

    disallowed = (revrangesep, '\r', '\n')
    for c in disallowed:
        if name.find(c) >= 0:
            raise util.Abort(_("%s cannot be used in a tag name") % repr(c))

    repo.hook('pretag', throw=True, node=r, tag=name,
              local=int(not not opts['local']))

    if opts['local']:
        repo.opener("localtags", "a").write("%s %s\n" % (r, name))
        repo.hook('tag', node=r, tag=name, local=1)
        return

    for x in repo.changes():
        if ".hgtags" in x:
            raise util.Abort(_("working copy of .hgtags is changed "
                               "(please commit .hgtags manually)"))

    repo.wfile(".hgtags", "ab").write("%s %s\n" % (r, name))
    if repo.dirstate.state(".hgtags") == '?':
        repo.add([".hgtags"])

    message = (opts['message'] or
               _("Added tag %s for changeset %s") % (name, r))
    try:
        repo.commit([".hgtags"], message, opts['user'], opts['date'])
        repo.hook('tag', node=r, tag=name, local=0)
    except ValueError, inst:
        raise util.Abort(str(inst))

def tags(ui, repo):
    """list repository tags

    List the repository tags.

    This lists both regular and local tags.
    """

    l = repo.tagslist()
    l.reverse()
    for t, n in l:
        try:
            r = "%5d:%s" % (repo.changelog.rev(n), hex(n))
        except KeyError:
            r = "    ?:?"
        ui.write("%-30s %s\n" % (t, r))

def tip(ui, repo, **opts):
    """show the tip revision

    Show the tip revision.
    """
    n = repo.changelog.tip()
    br = None
    if opts['branches']:
        br = repo.branchlookup([n])
    show_changeset(ui, repo, opts).show(changenode=n, brinfo=br)
    if opts['patch']:
        dodiff(ui, ui, repo, repo.changelog.parents(n)[0], n)

def unbundle(ui, repo, fname, **opts):
    """apply a changegroup file

    Apply a compressed changegroup file generated by the bundle
    command.
    """
    f = urllib.urlopen(fname)

    header = f.read(4)
    if header == "HG10":
        def generator(f):
            zd = bz2.BZ2Decompressor()
            for chunk in f:
                yield zd.decompress(chunk)
    elif header == "HG11":
        def generator(f):
            for chunk in f:
                yield chunk
    else:
        raise util.Abort(_("%s: not a Mercurial bundle file") % fname)
    gen = generator(util.filechunkiter(f, 4096))
    if repo.addchangegroup(util.chunkbuffer(gen)):
        return 1

    if opts['update']:
        return update(ui, repo)
    else:
        ui.status(_("(run 'hg update' to get a working copy)\n"))

def undo(ui, repo):
    """undo the last commit or pull

    Roll back the last pull or commit transaction on the
    repository, restoring the project to its earlier state.

    This command should be used with care. There is only one level of
    undo and there is no redo.

    This command is not intended for use on public repositories. Once
    a change is visible for pull by other users, undoing it locally is
    ineffective.
    """
    repo.undo()

def update(ui, repo, node=None, merge=False, clean=False, force=None,
           branch=None, **opts):
    """update or merge working directory

    Update the working directory to the specified revision.

    If there are no outstanding changes in the working directory and
    there is a linear relationship between the current version and the
    requested version, the result is the requested version.

    Otherwise the result is a merge between the contents of the
    current working directory and the requested version. Files that
    changed between either parent are marked as changed for the next
    commit and a commit must be performed before any further updates
    are allowed.

    By default, update will refuse to run if doing so would require
    merging or discarding local changes.
    """
    if branch:
        br = repo.branchlookup(branch=branch)
        found = []
        for x in br:
            if branch in br[x]:
                found.append(x)
        if len(found) > 1:
            ui.warn(_("Found multiple heads for %s\n") % branch)
            for x in found:
                show_changeset(ui, repo, opts).show(changenode=x, brinfo=br)
            return 1
        if len(found) == 1:
            node = found[0]
            ui.warn(_("Using head %s for branch %s\n") % (short(node), branch))
        else:
            ui.warn(_("branch %s not found\n") % (branch))
            return 1
    else:
        node = node and repo.lookup(node) or repo.changelog.tip()
    return repo.update(node, allow=merge, force=clean, forcemerge=force)

def verify(ui, repo):
    """verify the integrity of the repository

    Verify the integrity of the current repository.

    This will perform an extensive check of the repository's
    integrity, validating the hashes and checksums of each entry in
    the changelog, manifest, and tracked files, as well as the
    integrity of their crosslinks and indices.
    """
    return repo.verify()

# Command options and aliases are listed here, alphabetically

table = {
    "^add":
        (add,
         [('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg add [OPTION]... [FILE]...')),
    "addremove":
        (addremove,
         [('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg addremove [OPTION]... [FILE]...')),
    "^annotate":
        (annotate,
         [('r', 'rev', '', _('annotate the specified revision')),
          ('a', 'text', None, _('treat all files as text')),
          ('u', 'user', None, _('list the author')),
          ('d', 'date', None, _('list the date')),
          ('n', 'number', None, _('list the revision number (default)')),
          ('c', 'changeset', None, _('list the changeset')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg annotate [-r REV] [-a] [-u] [-d] [-n] [-c] FILE...')),
    "bundle":
        (bundle,
         [('f', 'force', None,
           _('run even when remote repository is unrelated'))],
         _('hg bundle FILE DEST')),
    "cat":
        (cat,
         [('o', 'output', '', _('print output to file with formatted name')),
          ('r', 'rev', '', _('print the given revision')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg cat [OPTION]... FILE...')),
    "^clone":
        (clone,
         [('U', 'noupdate', None, _('do not update the new working directory')),
          ('r', 'rev', [],
           _('a changeset you would like to have after cloning')),
          ('', 'pull', None, _('use pull protocol to copy metadata')),
          ('e', 'ssh', '', _('specify ssh command to use')),
          ('', 'remotecmd', '',
           _('specify hg command to run on the remote side'))],
         _('hg clone [OPTION]... SOURCE [DEST]')),
    "^commit|ci":
        (commit,
         [('A', 'addremove', None, _('run addremove during commit')),
          ('m', 'message', '', _('use <text> as commit message')),
          ('l', 'logfile', '', _('read the commit message from <file>')),
          ('d', 'date', '', _('record datecode as commit date')),
          ('u', 'user', '', _('record user as commiter')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg commit [OPTION]... [FILE]...')),
    "copy|cp":
        (copy,
         [('A', 'after', None, _('record a copy that has already occurred')),
          ('f', 'force', None,
           _('forcibly copy over an existing managed file')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg copy [OPTION]... [SOURCE]... DEST')),
    "debugancestor": (debugancestor, [], _('debugancestor INDEX REV1 REV2')),
    "debugcomplete": (debugcomplete, [], _('debugcomplete CMD')),
    "debugrebuildstate":
        (debugrebuildstate,
         [('r', 'rev', '', _('revision to rebuild to'))],
         _('debugrebuildstate [-r REV] [REV]')),
    "debugcheckstate": (debugcheckstate, [], _('debugcheckstate')),
    "debugconfig": (debugconfig, [], _('debugconfig')),
    "debugsetparents": (debugsetparents, [], _('debugsetparents REV1 [REV2]')),
    "debugstate": (debugstate, [], _('debugstate')),
    "debugdata": (debugdata, [], _('debugdata FILE REV')),
    "debugindex": (debugindex, [], _('debugindex FILE')),
    "debugindexdot": (debugindexdot, [], _('debugindexdot FILE')),
    "debugrename": (debugrename, [], _('debugrename FILE [REV]')),
    "debugwalk":
        (debugwalk,
         [('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('debugwalk [OPTION]... [FILE]...')),
    "^diff":
        (diff,
         [('r', 'rev', [], _('revision')),
          ('a', 'text', None, _('treat all files as text')),
          ('p', 'show-function', None,
           _('show which function each change is in')),
          ('w', 'ignore-all-space', None,
           _('ignore white space when comparing lines')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg diff [-a] [-I] [-X] [-r REV1 [-r REV2]] [FILE]...')),
    "^export":
        (export,
         [('o', 'output', '', _('print output to file with formatted name')),
          ('a', 'text', None, _('treat all files as text')),
          ('', 'switch-parent', None, _('diff against the second parent'))],
         _('hg export [-a] [-o OUTFILESPEC] REV...')),
    "forget":
        (forget,
         [('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg forget [OPTION]... FILE...')),
    "grep":
        (grep,
         [('0', 'print0', None, _('end fields with NUL')),
          ('', 'all', None, _('print all revisions that match')),
          ('i', 'ignore-case', None, _('ignore case when matching')),
          ('l', 'files-with-matches', None,
           _('print only filenames and revs that match')),
          ('n', 'line-number', None, _('print matching line numbers')),
          ('r', 'rev', [], _('search in given revision range')),
          ('u', 'user', None, _('print user who committed change')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg grep [OPTION]... PATTERN [FILE]...')),
    "heads":
        (heads,
         [('b', 'branches', None, _('show branches')),
          ('', 'style', '', _('display using template map file')),
          ('r', 'rev', '', _('show only heads which are descendants of rev')),
          ('', 'template', '', _('display with template'))],
         _('hg heads [-b] [-r <rev>]')),
    "help": (help_, [], _('hg help [COMMAND]')),
    "identify|id": (identify, [], _('hg identify')),
    "import|patch":
        (import_,
         [('p', 'strip', 1,
           _('directory strip option for patch. This has the same\n') +
           _('meaning as the corresponding patch option')),
          ('b', 'base', '', _('base path')),
          ('f', 'force', None,
           _('skip check for outstanding uncommitted changes'))],
         _('hg import [-p NUM] [-b BASE] [-f] PATCH...')),
    "incoming|in": (incoming,
         [('M', 'no-merges', None, _('do not show merges')),
          ('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('', 'style', '', _('display using template map file')),
          ('n', 'newest-first', None, _('show newest record first')),
          ('', 'bundle', '', _('file to store the bundles into')),
          ('p', 'patch', None, _('show patch')),
          ('', 'template', '', _('display with template')),
          ('e', 'ssh', '', _('specify ssh command to use')),
          ('', 'remotecmd', '',
           _('specify hg command to run on the remote side'))],
         _('hg incoming [-p] [-n] [-M] [--bundle FILENAME] [SOURCE]')),
    "^init": (init, [], _('hg init [DEST]')),
    "locate":
        (locate,
         [('r', 'rev', '', _('search the repository as it stood at rev')),
          ('0', 'print0', None,
           _('end filenames with NUL, for use with xargs')),
          ('f', 'fullpath', None,
           _('print complete paths from the filesystem root')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg locate [OPTION]... [PATTERN]...')),
    "^log|history":
        (log,
         [('b', 'branches', None, _('show branches')),
          ('k', 'keyword', [], _('search for a keyword')),
          ('l', 'limit', '', _('limit number of changes displayed')),
          ('r', 'rev', [], _('show the specified revision or range')),
          ('M', 'no-merges', None, _('do not show merges')),
          ('', 'style', '', _('display using template map file')),
          ('m', 'only-merges', None, _('show only merges')),
          ('p', 'patch', None, _('show patch')),
          ('', 'template', '', _('display with template')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg log [OPTION]... [FILE]')),
    "manifest": (manifest, [], _('hg manifest [REV]')),
    "outgoing|out": (outgoing,
         [('M', 'no-merges', None, _('do not show merges')),
          ('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('p', 'patch', None, _('show patch')),
          ('', 'style', '', _('display using template map file')),
          ('n', 'newest-first', None, _('show newest record first')),
          ('', 'template', '', _('display with template')),
          ('e', 'ssh', '', _('specify ssh command to use')),
          ('', 'remotecmd', '',
           _('specify hg command to run on the remote side'))],
         _('hg outgoing [-M] [-p] [-n] [DEST]')),
    "^parents":
        (parents,
         [('b', 'branches', None, _('show branches')),
          ('', 'style', '', _('display using template map file')),
          ('', 'template', '', _('display with template'))],
         _('hg parents [-b] [REV]')),
    "paths": (paths, [], _('hg paths [NAME]')),
    "^pull":
        (pull,
         [('u', 'update', None,
           _('update the working directory to tip after pull')),
          ('e', 'ssh', '', _('specify ssh command to use')),
          ('f', 'force', None,
           _('run even when remote repository is unrelated')),
          ('r', 'rev', [], _('a specific revision you would like to pull')),
          ('', 'remotecmd', '',
           _('specify hg command to run on the remote side'))],
         _('hg pull [-u] [-e FILE] [-r REV]... [--remotecmd FILE] [SOURCE]')),
    "^push":
        (push,
         [('f', 'force', None, _('force push')),
          ('e', 'ssh', '', _('specify ssh command to use')),
          ('r', 'rev', [], _('a specific revision you would like to push')),
          ('', 'remotecmd', '',
           _('specify hg command to run on the remote side'))],
         _('hg push [-f] [-e FILE] [-r REV]... [--remotecmd FILE] [DEST]')),
    "debugrawcommit|rawcommit":
        (rawcommit,
         [('p', 'parent', [], _('parent')),
          ('d', 'date', '', _('date code')),
          ('u', 'user', '', _('user')),
          ('F', 'files', '', _('file list')),
          ('m', 'message', '', _('commit message')),
          ('l', 'logfile', '', _('commit message file'))],
         _('hg debugrawcommit [OPTION]... [FILE]...')),
    "recover": (recover, [], _('hg recover')),
    "^remove|rm":
        (remove,
         [('f', 'force', None, _('remove file even if modified')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg remove [OPTION]... FILE...')),
    "rename|mv":
        (rename,
         [('A', 'after', None, _('record a rename that has already occurred')),
          ('f', 'force', None,
           _('forcibly copy over an existing managed file')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg rename [OPTION]... SOURCE... DEST')),
    "^revert":
        (revert,
         [('r', 'rev', '', _('revision to revert to')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg revert [-r REV] [NAME]...')),
    "root": (root, [], _('hg root')),
    "^serve":
        (serve,
         [('A', 'accesslog', '', _('name of access log file to write to')),
          ('d', 'daemon', None, _('run server in background')),
          ('', 'daemon-pipefds', '', _('used internally by daemon mode')),
          ('E', 'errorlog', '', _('name of error log file to write to')),
          ('p', 'port', 0, _('port to use (default: 8000)')),
          ('a', 'address', '', _('address to use')),
          ('n', 'name', '',
           _('name to show in web pages (default: working dir)')),
          ('', 'pid-file', '', _('name of file to write process ID to')),
          ('', 'stdio', None, _('for remote clients')),
          ('t', 'templates', '', _('web templates to use')),
          ('', 'style', '', _('template style to use')),
          ('6', 'ipv6', None, _('use IPv6 in addition to IPv4'))],
         _('hg serve [OPTION]...')),
    "^status|st":
        (status,
         [('m', 'modified', None, _('show only modified files')),
          ('a', 'added', None, _('show only added files')),
          ('r', 'removed', None, _('show only removed files')),
          ('d', 'deleted', None, _('show only deleted (but tracked) files')),
          ('u', 'unknown', None, _('show only unknown (not tracked) files')),
          ('n', 'no-status', None, _('hide status prefix')),
          ('0', 'print0', None,
           _('end filenames with NUL, for use with xargs')),
          ('I', 'include', [], _('include names matching the given patterns')),
          ('X', 'exclude', [], _('exclude names matching the given patterns'))],
         _('hg status [OPTION]... [FILE]...')),
    "tag":
        (tag,
         [('l', 'local', None, _('make the tag local')),
          ('m', 'message', '', _('message for tag commit log entry')),
          ('d', 'date', '', _('record datecode as commit date')),
          ('u', 'user', '', _('record user as commiter')),
          ('r', 'rev', '', _('revision to tag'))],
         _('hg tag [-l] [-m TEXT] [-d DATE] [-u USER] [-r REV] NAME')),
    "tags": (tags, [], _('hg tags')),
    "tip":
        (tip,
         [('b', 'branches', None, _('show branches')),
          ('', 'style', '', _('display using template map file')),
          ('p', 'patch', None, _('show patch')),
          ('', 'template', '', _('display with template'))],
         _('hg tip [-b] [-p]')),
    "unbundle":
        (unbundle,
         [('u', 'update', None,
           _('update the working directory to tip after unbundle'))],
         _('hg unbundle [-u] FILE')),
    "undo": (undo, [], _('hg undo')),
    "^update|up|checkout|co":
        (update,
         [('b', 'branch', '', _('checkout the head of a specific branch')),
          ('', 'style', '', _('display using template map file')),
          ('m', 'merge', None, _('allow merging of branches')),
          ('C', 'clean', None, _('overwrite locally modified files')),
          ('f', 'force', None, _('force a merge with outstanding changes')),
          ('', 'template', '', _('display with template'))],
         _('hg update [-b TAG] [-m] [-C] [-f] [REV]')),
    "verify": (verify, [], _('hg verify')),
    "version": (show_version, [], _('hg version')),
}

globalopts = [
    ('R', 'repository', '',
     _('repository root directory or symbolic path name')),
    ('', 'cwd', '', _('change working directory')),
    ('y', 'noninteractive', None,
     _('do not prompt, assume \'yes\' for any required answers')),
    ('q', 'quiet', None, _('suppress output')),
    ('v', 'verbose', None, _('enable additional output')),
    ('', 'debug', None, _('enable debugging output')),
    ('', 'debugger', None, _('start debugger')),
    ('', 'traceback', None, _('print traceback on exception')),
    ('', 'time', None, _('time how long the command takes')),
    ('', 'profile', None, _('print command execution profile')),
    ('', 'version', None, _('output version information and exit')),
    ('h', 'help', None, _('display help and exit')),
]

norepo = ("clone init version help debugancestor debugcomplete debugdata"
          " debugindex debugindexdot")
optionalrepo = ("paths debugconfig")

def findpossible(cmd):
    """
    Return cmd -> (aliases, command table entry)
    for each matching command
    """
    choice = {}
    debugchoice = {}
    for e in table.keys():
        aliases = e.lstrip("^").split("|")
        if cmd in aliases:
            choice[cmd] = (aliases, table[e])
            continue
        for a in aliases:
            if a.startswith(cmd):
                if aliases[0].startswith("debug"):
                    debugchoice[a] = (aliases, table[e])
                else:
                    choice[a] = (aliases, table[e])
                break

    if not choice and debugchoice:
        choice = debugchoice

    return choice

def find(cmd):
    """Return (aliases, command table entry) for command string."""
    choice = findpossible(cmd)

    if choice.has_key(cmd):
        return choice[cmd]

    if len(choice) > 1:
        clist = choice.keys()
        clist.sort()
        raise AmbiguousCommand(cmd, clist)

    if choice:
        return choice.values()[0]

    raise UnknownCommand(cmd)

class SignalInterrupt(Exception):
    """Exception raised on SIGTERM and SIGHUP."""

def catchterm(*args):
    raise SignalInterrupt

def run():
    sys.exit(dispatch(sys.argv[1:]))

class ParseError(Exception):
    """Exception raised on errors in parsing the command line."""

def parse(ui, args):
    options = {}
    cmdoptions = {}

    try:
        args = fancyopts.fancyopts(args, globalopts, options)
    except fancyopts.getopt.GetoptError, inst:
        raise ParseError(None, inst)

    if args:
        cmd, args = args[0], args[1:]
        aliases, i = find(cmd)
        cmd = aliases[0]
        defaults = ui.config("defaults", cmd)
        if defaults:
            args = defaults.split() + args
        c = list(i[1])
    else:
        cmd = None
        c = []

    # combine global options into local
    for o in globalopts:
        c.append((o[0], o[1], options[o[1]], o[3]))

    try:
        args = fancyopts.fancyopts(args, c, cmdoptions)
    except fancyopts.getopt.GetoptError, inst:
        raise ParseError(cmd, inst)

    # separate global options back out
    for o in globalopts:
        n = o[1]
        options[n] = cmdoptions[n]
        del cmdoptions[n]

    return (cmd, cmd and i[0] or None, args, options, cmdoptions)

def dispatch(args):
    signal.signal(signal.SIGTERM, catchterm)
    try:
        signal.signal(signal.SIGHUP, catchterm)
    except AttributeError:
        pass

    try:
        u = ui.ui()
    except util.Abort, inst:
        sys.stderr.write(_("abort: %s\n") % inst)
        sys.exit(1)

    external = []
    for x in u.extensions():
        def on_exception(exc, inst):
            u.warn(_("*** failed to import extension %s\n") % x[1])
            u.warn("%s\n" % inst)
            if "--traceback" in sys.argv[1:]:
                traceback.print_exc()
        if x[1]:
            try:
                mod = imp.load_source(x[0], x[1])
            except Exception, inst:
                on_exception(Exception, inst)
                continue
        else:
            def importh(name):
                mod = __import__(name)
                components = name.split('.')
                for comp in components[1:]:
                    mod = getattr(mod, comp)
                return mod
            try:
                try:
                    mod = importh("hgext." + x[0])
                except ImportError:
                    mod = importh(x[0])
            except Exception, inst:
                on_exception(Exception, inst)
                continue

        external.append(mod)
    for x in external:
        cmdtable = getattr(x, 'cmdtable', {})
        for t in cmdtable:
            if t in table:
                u.warn(_("module %s overrides %s\n") % (x.__name__, t))
        table.update(cmdtable)

    try:
        cmd, func, args, options, cmdoptions = parse(u, args)
        if options["time"]:
            def get_times():
                t = os.times()
                if t[4] == 0.0: # Windows leaves this as zero, so use time.clock()
                    t = (t[0], t[1], t[2], t[3], time.clock())
                return t
            s = get_times()
            def print_time():
                t = get_times()
                u.warn(_("Time: real %.3f secs (user %.3f+%.3f sys %.3f+%.3f)\n") %
                    (t[4]-s[4], t[0]-s[0], t[2]-s[2], t[1]-s[1], t[3]-s[3]))
            atexit.register(print_time)

        u.updateopts(options["verbose"], options["debug"], options["quiet"],
                not options["noninteractive"])

        # enter the debugger before command execution
        if options['debugger']:
            pdb.set_trace()

        try:
            if options['cwd']:
                try:
                    os.chdir(options['cwd'])
                except OSError, inst:
                    raise util.Abort('%s: %s' %
                                     (options['cwd'], inst.strerror))

            path = u.expandpath(options["repository"]) or ""
            repo = path and hg.repository(u, path=path) or None

            if options['help']:
                help_(u, cmd, options['version'])
                sys.exit(0)
            elif options['version']:
                show_version(u)
                sys.exit(0)
            elif not cmd:
                help_(u, 'shortlist')
                sys.exit(0)

            if cmd not in norepo.split():
                try:
                    if not repo:
                        repo = hg.repository(u, path=path)
                    u = repo.ui
                    for x in external:
                        if hasattr(x, 'reposetup'):
                            x.reposetup(u, repo)
                except hg.RepoError:
                    if cmd not in optionalrepo.split():
                        raise
                d = lambda: func(u, repo, *args, **cmdoptions)
            else:
                d = lambda: func(u, *args, **cmdoptions)

            try:
                if options['profile']:
                    import hotshot, hotshot.stats
                    prof = hotshot.Profile("hg.prof")
                    try:
                        try:
                            return prof.runcall(d)
                        except:
                            try:
                                u.warn(_('exception raised - generating '
                                         'profile anyway\n'))
                            except:
                                pass
                            raise
                    finally:
                        prof.close()
                        stats = hotshot.stats.load("hg.prof")
                        stats.strip_dirs()
                        stats.sort_stats('time', 'calls')
                        stats.print_stats(40)
                else:
                    return d()
            finally:
                u.flush()
        except:
            # enter the debugger when we hit an exception
            if options['debugger']:
                pdb.post_mortem(sys.exc_info()[2])
            if options['traceback']:
                traceback.print_exc()
            raise
    except ParseError, inst:
        if inst.args[0]:
            u.warn(_("hg %s: %s\n") % (inst.args[0], inst.args[1]))
            help_(u, inst.args[0])
        else:
            u.warn(_("hg: %s\n") % inst.args[1])
            help_(u, 'shortlist')
        sys.exit(-1)
    except AmbiguousCommand, inst:
        u.warn(_("hg: command '%s' is ambiguous:\n    %s\n") %
                (inst.args[0], " ".join(inst.args[1])))
        sys.exit(1)
    except UnknownCommand, inst:
        u.warn(_("hg: unknown command '%s'\n") % inst.args[0])
        help_(u, 'shortlist')
        sys.exit(1)
    except hg.RepoError, inst:
        u.warn(_("abort: "), inst, "!\n")
    except revlog.RevlogError, inst:
        u.warn(_("abort: "), inst, "!\n")
    except SignalInterrupt:
        u.warn(_("killed!\n"))
    except KeyboardInterrupt:
        try:
            u.warn(_("interrupted!\n"))
        except IOError, inst:
            if inst.errno == errno.EPIPE:
                if u.debugflag:
                    u.warn(_("\nbroken pipe\n"))
            else:
                raise
    except IOError, inst:
        if hasattr(inst, "code"):
            u.warn(_("abort: %s\n") % inst)
        elif hasattr(inst, "reason"):
            u.warn(_("abort: error: %s\n") % inst.reason[1])
        elif hasattr(inst, "args") and inst[0] == errno.EPIPE:
            if u.debugflag:
                u.warn(_("broken pipe\n"))
        elif getattr(inst, "strerror", None):
            if getattr(inst, "filename", None):
                u.warn(_("abort: %s - %s\n") % (inst.strerror, inst.filename))
            else:
                u.warn(_("abort: %s\n") % inst.strerror)
        else:
            raise
    except OSError, inst:
        if hasattr(inst, "filename"):
            u.warn(_("abort: %s: %s\n") % (inst.strerror, inst.filename))
        else:
            u.warn(_("abort: %s\n") % inst.strerror)
    except util.Abort, inst:
        u.warn(_('abort: '), inst.args[0] % inst.args[1:], '\n')
        sys.exit(1)
    except TypeError, inst:
        # was this an argument error?
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) > 2: # no
            raise
        u.debug(inst, "\n")
        u.warn(_("%s: invalid arguments\n") % cmd)
        help_(u, cmd)
    except SystemExit:
        # don't catch this in the catch-all below
        raise
    except:
        u.warn(_("** unknown exception encountered, details follow\n"))
        u.warn(_("** report bug details to mercurial@selenic.com\n"))
        u.warn(_("** Mercurial Distributed SCM (version %s)\n")
               % version.get_version())
        raise

    sys.exit(-1)
