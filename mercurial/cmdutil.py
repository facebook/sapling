# cmdutil.py - help for command processing in mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
from node import *
from i18n import gettext as _
demandload(globals(), 'os sys')
demandload(globals(), 'mdiff util templater cStringIO patch')

revrangesep = ':'

def revpair(ui, repo, revs):
    '''return pair of nodes, given list of revisions. second item can
    be None, meaning use working dir.'''

    def revfix(repo, val, defval):
        if not val and val != 0:
            val = defval
        return repo.lookup(val)

    if not revs:
        return repo.dirstate.parents()[0], None
    end = None
    if len(revs) == 1:
        if revrangesep in revs[0]:
            start, end = revs[0].split(revrangesep, 1)
            start = revfix(repo, start, 0)
            end = revfix(repo, end, repo.changelog.count() - 1)
        else:
            start = revfix(repo, revs[0], None)
    elif len(revs) == 2:
        if revrangesep in revs[0] or revrangesep in revs[1]:
            raise util.Abort(_('too many revisions specified'))
        start = revfix(repo, revs[0], None)
        end = revfix(repo, revs[1], None)
    else:
        raise util.Abort(_('too many revisions specified'))
    return start, end

def revrange(ui, repo, revs):
    """Yield revision as strings from a list of revision specifications."""

    def revfix(repo, val, defval):
        if not val and val != 0:
            return defval
        return repo.changelog.rev(repo.lookup(val))

    seen, l = {}, []
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
                l.append(rev)
        else:
            rev = revfix(repo, spec, None)
            if rev in seen:
                continue
            seen[rev] = 1
            l.append(rev)

    return l

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

def walk(repo, pats=[], opts={}, node=None, head='', badmatch=None):
    files, matchfn, anypats = matchpats(repo, pats, opts, head)
    exact = dict.fromkeys(files)
    for src, fn in repo.walk(node=node, files=files, match=matchfn,
                             badmatch=badmatch):
        yield src, fn, util.pathto(repo.getcwd(), fn), fn in exact

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

class uibuffer(object):
    # Implement and delegate some ui protocol.  Save hunks of
    # output for later display in the desired order.
    def __init__(self, ui):
        self.ui = ui
        self.hunk = {}
        self.header = {}
        self.quiet = ui.quiet
        self.verbose = ui.verbose
        self.debugflag = ui.debugflag
        self.lastheader = None
    def note(self, *args):
        if self.verbose:
            self.write(*args)
    def status(self, *args):
        if not self.quiet:
            self.write(*args)
    def debug(self, *args):
        if self.debugflag:
            self.write(*args)
    def write(self, *args):
        self.hunk.setdefault(self.rev, []).extend(args)
    def write_header(self, *args):
        self.header.setdefault(self.rev, []).extend(args)
    def mark(self, rev):
        self.rev = rev
    def flush(self, rev):
        if rev in self.header:
            h = "".join(self.header[rev])
            if h != self.lastheader:
                self.lastheader = h
                self.ui.write(h)
            del self.header[rev]
        if rev in self.hunk:
            self.ui.write("".join(self.hunk[rev]))
            del self.hunk[rev]
            return 1
        return 0

class changeset_printer(object):
    '''show changeset information when templating not requested.'''

    def __init__(self, ui, repo, patch, brinfo, buffered):
        self.ui = ui
        self.repo = repo
        self.buffered = buffered
        self.patch = patch
        self.brinfo = brinfo
        if buffered:
            self.ui = uibuffer(ui)

    def flush(self, rev):
        return self.ui.flush(rev)

    def show(self, rev=0, changenode=None, copies=None):
        '''show a single changeset or file revision'''
        if self.buffered:
            self.ui.mark(rev)
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
        extra = changes[5]
        branch = extra.get("branch")

        hexfunc = self.ui.debugflag and hex or short

        parents = log.parentrevs(rev)
        if not self.ui.debugflag:
            if parents[1] == nullrev:
                if parents[0] >= rev - 1:
                    parents = []
                else:
                    parents = [parents[0]]
        parents = [(p, hexfunc(log.node(p))) for p in parents]

        self.ui.write(_("changeset:   %d:%s\n") % (rev, hexfunc(changenode)))

        if branch:
            self.ui.write(_("branch:      %s\n") % branch)
        for tag in self.repo.nodetags(changenode):
            self.ui.write(_("tag:         %s\n") % tag)
        for parent in parents:
            self.ui.write(_("parent:      %d:%s\n") % parent)

        if self.brinfo:
            br = self.repo.branchlookup([changenode])
            if br:
                self.ui.write(_("branch:      %s\n") % " ".join(br[changenode]))

        if self.ui.debugflag:
            self.ui.write(_("manifest:    %d:%s\n") %
                          (self.repo.manifest.rev(changes[0]), hex(changes[0])))
        self.ui.write(_("user:        %s\n") % changes[1])
        self.ui.write(_("date:        %s\n") % date)

        if self.ui.debugflag:
            files = self.repo.status(log.parents(changenode)[0], changenode)[:3]
            for key, value in zip([_("files:"), _("files+:"), _("files-:")],
                                  files):
                if value:
                    self.ui.write("%-12s %s\n" % (key, " ".join(value)))
        elif changes[3] and self.ui.verbose:
            self.ui.write(_("files:       %s\n") % " ".join(changes[3]))
        if copies and self.ui.verbose:
            copies = ['%s (%s)' % c for c in copies]
            self.ui.write(_("copies:      %s\n") % ' '.join(copies))

        if extra and self.ui.debugflag:
            extraitems = extra.items()
            extraitems.sort()
            for key, value in extraitems:
                self.ui.write(_("extra:       %s=%s\n")
                              % (key, value.encode('string_escape')))

        description = changes[4].strip()
        if description:
            if self.ui.verbose:
                self.ui.write(_("description:\n"))
                self.ui.write(description)
                self.ui.write("\n\n")
            else:
                self.ui.write(_("summary:     %s\n") %
                              description.splitlines()[0])
        self.ui.write("\n")

        self.showpatch(changenode)

    def showpatch(self, node):
        if self.patch:
            prev = self.repo.changelog.parents(node)[0]
            patch.diff(self.repo, prev, node, fp=self.ui)
            self.ui.write("\n")

class changeset_templater(changeset_printer):
    '''format changeset information.'''

    def __init__(self, ui, repo, patch, brinfo, mapfile, buffered):
        changeset_printer.__init__(self, ui, repo, patch, brinfo, buffered)
        self.t = templater.templater(mapfile, templater.common_filters,
                                     cache={'parent': '{rev}:{node|short} ',
                                            'manifest': '{rev}:{node|short}',
                                            'filecopy': '{name} ({source})'})

    def use_template(self, t):
        '''set template string to use'''
        self.t.cache['changeset'] = t

    def show(self, rev=0, changenode=None, copies=[], **props):
        '''show a single changeset or file revision'''
        if self.buffered:
            self.ui.mark(rev)
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

        def showbranches(**args):
            branch = changes[5].get("branch")
            if branch:
                return showlist('branch', [branch], plural='branches', **args)
            # add old style branches if requested
            if self.brinfo:
                br = self.repo.branchlookup([changenode])
                if changenode in br:
                    return showlist('branch', br[changenode],
                                    plural='branches', **args)

        def showparents(**args):
            parents = [[('rev', log.rev(p)), ('node', hex(p))]
                       for p in log.parents(changenode)
                       if self.ui.debugflag or p != nullid]
            if (not self.ui.debugflag and len(parents) == 1 and
                parents[0][0][1] == rev - 1):
                return
            return showlist('parent', parents, **args)

        def showtags(**args):
            return showlist('tag', self.repo.nodetags(changenode), **args)

        def showextras(**args):
            extras = changes[5].items()
            extras.sort()
            for key, value in extras:
                args = args.copy()
                args.update(dict(key=key, value=value))
                yield self.t('extra', **args)

        def showcopies(**args):
            c = [{'name': x[0], 'source': x[1]} for x in copies]
            return showlist('file_copy', c, plural='file_copies', **args)

        if self.ui.debugflag:
            files = self.repo.status(log.parents(changenode)[0], changenode)[:3]
            def showfiles(**args):
                return showlist('file', files[0], **args)
            def showadds(**args):
                return showlist('file_add', files[1], **args)
            def showdels(**args):
                return showlist('file_del', files[2], **args)
            def showmanifest(**args):
                args = args.copy()
                args.update(dict(rev=self.repo.manifest.rev(changes[0]),
                                 node=hex(changes[0])))
                return self.t('manifest', **args)
        else:
            def showfiles(**args):
                return showlist('file', changes[3], **args)
            showadds = ''
            showdels = ''
            showmanifest = ''

        defprops = {
            'author': changes[1],
            'branches': showbranches,
            'date': changes[2],
            'desc': changes[4],
            'file_adds': showadds,
            'file_dels': showdels,
            'files': showfiles,
            'file_copies': showcopies,
            'manifest': showmanifest,
            'node': hex(changenode),
            'parents': showparents,
            'rev': rev,
            'tags': showtags,
            'extras': showextras,
            }
        props = props.copy()
        props.update(defprops)

        try:
            if self.ui.debugflag and 'header_debug' in self.t:
                key = 'header_debug'
            elif self.ui.quiet and 'header_quiet' in self.t:
                key = 'header_quiet'
            elif self.ui.verbose and 'header_verbose' in self.t:
                key = 'header_verbose'
            elif 'header' in self.t:
                key = 'header'
            else:
                key = ''
            if key:
                h = templater.stringify(self.t(key, **props))
                if self.buffered:
                    self.ui.write_header(h)
                else:
                    self.ui.write(h)
            if self.ui.debugflag and 'changeset_debug' in self.t:
                key = 'changeset_debug'
            elif self.ui.quiet and 'changeset_quiet' in self.t:
                key = 'changeset_quiet'
            elif self.ui.verbose and 'changeset_verbose' in self.t:
                key = 'changeset_verbose'
            else:
                key = 'changeset'
            self.ui.write(templater.stringify(self.t(key, **props)))
            self.showpatch(changenode)
        except KeyError, inst:
            raise util.Abort(_("%s: no key named '%s'") % (self.t.mapfile,
                                                           inst.args[0]))
        except SyntaxError, inst:
            raise util.Abort(_('%s: %s') % (self.t.mapfile, inst.args[0]))

class stringio(object):
    '''wrap cStringIO for use by changeset_templater.'''
    def __init__(self):
        self.fp = cStringIO.StringIO()

    def write(self, *args):
        for a in args:
            self.fp.write(a)

    write_header = write

    def __getattr__(self, key):
        return getattr(self.fp, key)

def show_changeset(ui, repo, opts, buffered=False):
    """show one changeset using template or regular display.

    Display format will be the first non-empty hit of:
    1. option 'template'
    2. option 'style'
    3. [ui] setting 'logtemplate'
    4. [ui] setting 'style'
    If all of these values are either the unset or the empty string,
    regular display via changeset_printer() is done.
    """
    # options
    patch = opts.get('patch')
    br = None
    if opts.get('branches'):
        ui.warn(_("the --branches option is deprecated, "
                  "please use 'hg branches' instead\n"))
        br = True
    tmpl = opts.get('template')
    mapfile = None
    if tmpl:
        tmpl = templater.parsestring(tmpl, quoted=False)
    else:
        mapfile = opts.get('style')
        # ui settings
        if not mapfile:
            tmpl = ui.config('ui', 'logtemplate')
            if tmpl:
                tmpl = templater.parsestring(tmpl)
            else:
                mapfile = ui.config('ui', 'style')

    if tmpl or mapfile:
        if mapfile:
            if not os.path.split(mapfile)[0]:
                mapname = (templater.templatepath('map-cmdline.' + mapfile)
                           or templater.templatepath(mapfile))
                if mapname: mapfile = mapname
        try:
            t = changeset_templater(ui, repo, patch, br, mapfile, buffered)
        except SyntaxError, inst:
            raise util.Abort(inst.args[0])
        if tmpl: t.use_template(tmpl)
        return t
    return changeset_printer(ui, repo, patch, br, buffered)

