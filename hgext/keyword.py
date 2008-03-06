# keyword.py - $Keyword$ expansion for Mercurial
#
# Copyright 2007, 2008 Christian Ebert <blacktrash@gmx.net>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# $Id$
#
# Keyword expansion hack against the grain of a DSCM
#
# There are many good reasons why this is not needed in a distributed
# SCM, still it may be useful in very small projects based on single
# files (like LaTeX packages), that are mostly addressed to an audience
# not running a version control system.
#
# For in-depth discussion refer to
# <http://www.selenic.com/mercurial/wiki/index.cgi/KeywordPlan>.
#
# Keyword expansion is based on Mercurial's changeset template mappings.
#
# Binary files are not touched.
#
# Setup in hgrc:
#
#   [extensions]
#   # enable extension
#   hgext.keyword =
#
# Files to act upon/ignore are specified in the [keyword] section.
# Customized keyword template mappings in the [keywordmaps] section.
#
# Run "hg help keyword" and "hg kwdemo" to get info on configuration.

'''keyword expansion in local repositories

This extension expands RCS/CVS-like or self-customized $Keywords$
in tracked text files selected by your configuration.

Keywords are only expanded in local repositories and not stored in
the change history. The mechanism can be regarded as a convenience
for the current user or for archive distribution.

Configuration is done in the [keyword] and [keywordmaps] sections
of hgrc files.

Example:

    [keyword]
    # expand keywords in every python file except those matching "x*"
    **.py =
    x*    = ignore

Note: the more specific you are in your filename patterns
      the less you lose speed in huge repos.

For [keywordmaps] template mapping and expansion demonstration and
control run "hg kwdemo".

An additional date template filter {date|utcdate} is provided.

The default template mappings (view with "hg kwdemo -d") can be replaced
with customized keywords and templates.
Again, run "hg kwdemo" to control the results of your config changes.

Before changing/disabling active keywords, run "hg kwshrink" to avoid
the risk of inadvertedly storing expanded keywords in the change history.

To force expansion after enabling it, or a configuration change, run
"hg kwexpand".

Also, when committing with the record extension or using mq's qrecord, be aware
that keywords cannot be updated. Again, run "hg kwexpand" on the files in
question to update keyword expansions after all changes have been checked in.

Expansions spanning more than one line and incremental expansions,
like CVS' $Log$, are not supported. A keyword template map
"Log = {desc}" expands to the first line of the changeset description.
'''

from mercurial import commands, cmdutil, context, dispatch, filelog, revlog
from mercurial import patch, localrepo, templater, templatefilters, util
from mercurial.hgweb import webcommands
from mercurial.node import nullid, hex
from mercurial.i18n import _
import re, shutil, tempfile, time

commands.optionalrepo += ' kwdemo'

# hg commands that do not act on keywords
nokwcommands = ('add addremove bundle copy export grep incoming init'
                ' log outgoing push rename rollback tip'
                ' convert email glog')

# hg commands that trigger expansion only when writing to working dir,
# not when reading filelog, and unexpand when reading from working dir
restricted = 'record qfold qimport qnew qpush qrefresh qrecord'

def utcdate(date):
    '''Returns hgdate in cvs-like UTC format.'''
    return time.strftime('%Y/%m/%d %H:%M:%S', time.gmtime(date[0]))


# make keyword tools accessible
kwtools = {'templater': None, 'hgcmd': None}

# store originals of monkeypatches
_patchfile_init = patch.patchfile.__init__
_patch_diff = patch.diff
_dispatch_parse = dispatch._parse

def _kwpatchfile_init(self, ui, fname, missing=False):
    '''Monkeypatch/wrap patch.patchfile.__init__ to avoid
    rejects or conflicts due to expanded keywords in working dir.'''
    _patchfile_init(self, ui, fname, missing=missing)
    # shrink keywords read from working dir
    kwt = kwtools['templater']
    self.lines = kwt.shrinklines(self.fname, self.lines)

def _kw_diff(repo, node1=None, node2=None, files=None, match=util.always,
             fp=None, changes=None, opts=None):
    '''Monkeypatch patch.diff to avoid expansion except when
    comparing against working dir.'''
    if node2 is not None:
        kwtools['templater'].matcher = util.never
    elif node1 is not None and node1 != repo.changectx().node():
        kwtools['templater'].restrict = True
    _patch_diff(repo, node1=node1, node2=node2, files=files, match=match,
                fp=fp, changes=changes, opts=opts)

def _kwweb_changeset(web, req, tmpl):
    '''Wraps webcommands.changeset turning off keyword expansion.'''
    kwtools['templater'].matcher = util.never
    return web.changeset(tmpl, web.changectx(req))

def _kwweb_filediff(web, req, tmpl):
    '''Wraps webcommands.filediff turning off keyword expansion.'''
    kwtools['templater'].matcher = util.never
    return web.filediff(tmpl, web.filectx(req))

def _kwdispatch_parse(ui, args):
    '''Monkeypatch dispatch._parse to obtain running hg command.'''
    cmd, func, args, options, cmdoptions = _dispatch_parse(ui, args)
    kwtools['hgcmd'] = cmd
    return cmd, func, args, options, cmdoptions

# dispatch._parse is run before reposetup, so wrap it here
dispatch._parse = _kwdispatch_parse


class kwtemplater(object):
    '''
    Sets up keyword templates, corresponding keyword regex, and
    provides keyword substitution functions.
    '''
    templates = {
        'Revision': '{node|short}',
        'Author': '{author|user}',
        'Date': '{date|utcdate}',
        'RCSFile': '{file|basename},v',
        'Source': '{root}/{file},v',
        'Id': '{file|basename},v {node|short} {date|utcdate} {author|user}',
        'Header': '{root}/{file},v {node|short} {date|utcdate} {author|user}',
    }

    def __init__(self, ui, repo, inc, exc):
        self.ui = ui
        self.repo = repo
        self.matcher = util.matcher(repo.root, inc=inc, exc=exc)[1]
        self.restrict = kwtools['hgcmd'] in restricted.split()

        kwmaps = self.ui.configitems('keywordmaps')
        if kwmaps: # override default templates
            kwmaps = [(k, templater.parsestring(v, quoted=False))
                      for (k, v) in kwmaps]
            self.templates = dict(kwmaps)
        escaped = map(re.escape, self.templates.keys())
        kwpat = r'\$(%s)(: [^$\n\r]*? )??\$' % '|'.join(escaped)
        self.re_kw = re.compile(kwpat)

        templatefilters.filters['utcdate'] = utcdate
        self.ct = cmdutil.changeset_templater(self.ui, self.repo,
                                              False, '', False)

    def getnode(self, path, fnode):
        '''Derives changenode from file path and filenode.'''
        # used by kwfilelog.read and kwexpand
        c = context.filectx(self.repo, path, fileid=fnode)
        return c.node()

    def substitute(self, data, path, node, subfunc):
        '''Replaces keywords in data with expanded template.'''
        def kwsub(mobj):
            kw = mobj.group(1)
            self.ct.use_template(self.templates[kw])
            self.ui.pushbuffer()
            self.ct.show(changenode=node, root=self.repo.root, file=path)
            ekw = templatefilters.firstline(self.ui.popbuffer())
            return '$%s: %s $' % (kw, ekw)
        return subfunc(kwsub, data)

    def expand(self, path, node, data):
        '''Returns data with keywords expanded.'''
        if not self.restrict and self.matcher(path) and not util.binary(data):
            changenode = self.getnode(path, node)
            return self.substitute(data, path, changenode, self.re_kw.sub)
        return data

    def iskwfile(self, path, islink):
        '''Returns true if path matches [keyword] pattern
        and is not a symbolic link.
        Caveat: localrepository._link fails on Windows.'''
        return self.matcher(path) and not islink(path)

    def overwrite(self, node=None, expand=True, files=None):
        '''Overwrites selected files expanding/shrinking keywords.'''
        ctx = self.repo.changectx(node)
        mf = ctx.manifest()
        if node is not None:     # commit
            files = [f for f in ctx.files() if f in mf]
            notify = self.ui.debug
        else:                    # kwexpand/kwshrink
            notify = self.ui.note
        candidates = [f for f in files if self.iskwfile(f, mf.linkf)]
        if candidates:
            self.restrict = True # do not expand when reading
            candidates.sort()
            action = expand and 'expanding' or 'shrinking'
            for f in candidates:
                fp = self.repo.file(f)
                data = fp.read(mf[f])
                if util.binary(data):
                    continue
                if expand:
                    changenode = node or self.getnode(f, mf[f])
                    data, found = self.substitute(data, f, changenode,
                                                  self.re_kw.subn)
                else:
                    found = self.re_kw.search(data)
                if found:
                    notify(_('overwriting %s %s keywords\n') % (f, action))
                    self.repo.wwrite(f, data, mf.flags(f))
                    self.repo.dirstate.normal(f)
            self.restrict = False

    def shrinktext(self, text):
        '''Unconditionally removes all keyword substitutions from text.'''
        return self.re_kw.sub(r'$\1$', text)

    def shrink(self, fname, text):
        '''Returns text with all keyword substitutions removed.'''
        if self.matcher(fname) and not util.binary(text):
            return self.shrinktext(text)
        return text

    def shrinklines(self, fname, lines):
        '''Returns lines with keyword substitutions removed.'''
        if self.matcher(fname):
            text = ''.join(lines)
            if not util.binary(text):
                return self.shrinktext(text).splitlines(True)
        return lines

    def wread(self, fname, data):
        '''If in restricted mode returns data read from wdir with
        keyword substitutions removed.'''
        return self.restrict and self.shrink(fname, data) or data

class kwfilelog(filelog.filelog):
    '''
    Subclass of filelog to hook into its read, add, cmp methods.
    Keywords are "stored" unexpanded, and processed on reading.
    '''
    def __init__(self, opener, path):
        super(kwfilelog, self).__init__(opener, path)
        self.kwt = kwtools['templater']
        self.path = path

    def read(self, node):
        '''Expands keywords when reading filelog.'''
        data = super(kwfilelog, self).read(node)
        return self.kwt.expand(self.path, node, data)

    def add(self, text, meta, tr, link, p1=None, p2=None):
        '''Removes keyword substitutions when adding to filelog.'''
        text = self.kwt.shrink(self.path, text)
        return super(kwfilelog, self).add(text, meta, tr, link, p1=p1, p2=p2)

    def cmp(self, node, text):
        '''Removes keyword substitutions for comparison.'''
        text = self.kwt.shrink(self.path, text)
        if self.renamed(node):
            t2 = super(kwfilelog, self).read(node)
            return t2 != text
        return revlog.revlog.cmp(self, node, text)

def _status(ui, repo, kwt, *pats, **opts):
    '''Bails out if [keyword] configuration is not active.
    Returns status of working directory.'''
    if kwt:
        files, match, anypats = cmdutil.matchpats(repo, pats, opts)
        return repo.status(files=files, match=match, list_clean=True)
    if ui.configitems('keyword'):
        raise util.Abort(_('[keyword] patterns cannot match'))
    raise util.Abort(_('no [keyword] patterns configured'))

def _kwfwrite(ui, repo, expand, *pats, **opts):
    '''Selects files and passes them to kwtemplater.overwrite.'''
    kwt = kwtools['templater']
    status = _status(ui, repo, kwt, *pats, **opts)
    modified, added, removed, deleted, unknown, ignored, clean = status
    if modified or added or removed or deleted:
        raise util.Abort(_('outstanding uncommitted changes in given files'))
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        kwt.overwrite(expand=expand, files=clean)
    finally:
        del wlock, lock


def demo(ui, repo, *args, **opts):
    '''print [keywordmaps] configuration and an expansion example

    Show current, custom, or default keyword template maps
    and their expansion.

    Extend current configuration by specifying maps as arguments
    and optionally by reading from an additional hgrc file.

    Override current keyword template maps with "default" option.
    '''
    def demostatus(stat):
        ui.status(_('\n\t%s\n') % stat)

    def demoitems(section, items):
        ui.write('[%s]\n' % section)
        for k, v in items:
            ui.write('%s = %s\n' % (k, v))

    msg = 'hg keyword config and expansion example'
    kwstatus = 'current'
    fn = 'demo.txt'
    branchname = 'demobranch'
    tmpdir = tempfile.mkdtemp('', 'kwdemo.')
    ui.note(_('creating temporary repo at %s\n') % tmpdir)
    repo = localrepo.localrepository(ui, path=tmpdir, create=True)
    ui.setconfig('keyword', fn, '')
    if args or opts.get('rcfile'):
        kwstatus = 'custom'
    if opts.get('rcfile'):
        ui.readconfig(opts.get('rcfile'))
    if opts.get('default'):
        kwstatus = 'default'
        kwmaps = kwtemplater.templates
        if ui.configitems('keywordmaps'):
            # override maps from optional rcfile
            for k, v in kwmaps.iteritems():
                ui.setconfig('keywordmaps', k, v)
    elif args:
        # simulate hgrc parsing
        rcmaps = ['[keywordmaps]\n'] + [a + '\n' for a in args]
        fp = repo.opener('hgrc', 'w')
        fp.writelines(rcmaps)
        fp.close()
        ui.readconfig(repo.join('hgrc'))
    if not opts.get('default'):
        kwmaps = dict(ui.configitems('keywordmaps')) or kwtemplater.templates
    reposetup(ui, repo)
    for k, v in ui.configitems('extensions'):
        if k.endswith('keyword'):
            extension = '%s = %s' % (k, v)
            break
    demostatus('config using %s keyword template maps' % kwstatus)
    ui.write('[extensions]\n%s\n' % extension)
    demoitems('keyword', ui.configitems('keyword'))
    demoitems('keywordmaps', kwmaps.iteritems())
    keywords = '$' + '$\n$'.join(kwmaps.keys()) + '$\n'
    repo.wopener(fn, 'w').write(keywords)
    repo.add([fn])
    path = repo.wjoin(fn)
    ui.note(_('\n%s keywords written to %s:\n') % (kwstatus, path))
    ui.note(keywords)
    ui.note('\nhg -R "%s" branch "%s"\n' % (tmpdir, branchname))
    # silence branch command if not verbose
    quiet = ui.quiet
    ui.quiet = not ui.verbose
    commands.branch(ui, repo, branchname)
    ui.quiet = quiet
    for name, cmd in ui.configitems('hooks'):
        if name.split('.', 1)[0].find('commit') > -1:
            repo.ui.setconfig('hooks', name, '')
    ui.note(_('unhooked all commit hooks\n'))
    ui.note('hg -R "%s" ci -m "%s"\n' % (tmpdir, msg))
    repo.commit(text=msg)
    format = ui.verbose and ' in %s' % path or ''
    demostatus('%s keywords expanded%s' % (kwstatus, format))
    ui.write(repo.wread(fn))
    ui.debug(_('\nremoving temporary repo %s\n') % tmpdir)
    shutil.rmtree(tmpdir, ignore_errors=True)

def expand(ui, repo, *pats, **opts):
    '''expand keywords in working directory

    Run after (re)enabling keyword expansion.

    kwexpand refuses to run if given files contain local changes.
    '''
    # 3rd argument sets expansion to True
    _kwfwrite(ui, repo, True, *pats, **opts)

def files(ui, repo, *pats, **opts):
    '''print files currently configured for keyword expansion

    Crosscheck which files in working directory are potential targets for
    keyword expansion.
    That is, files matched by [keyword] config patterns but not symlinks.
    '''
    kwt = kwtools['templater']
    status = _status(ui, repo, kwt, *pats, **opts)
    modified, added, removed, deleted, unknown, ignored, clean = status
    files = modified + added + clean
    if opts.get('untracked'):
        files += unknown
    files.sort()
    wctx = repo.workingctx()
    islink = lambda p: 'l' in wctx.fileflags(p)
    kwfiles = [f for f in files if kwt.iskwfile(f, islink)]
    cwd = pats and repo.getcwd() or ''
    kwfstats = not opts.get('ignore') and (('K', kwfiles),) or ()
    if opts.get('all') or opts.get('ignore'):
        kwfstats += (('I', [f for f in files if f not in kwfiles]),)
    for char, filenames in kwfstats:
        format = (opts.get('all') or ui.verbose) and '%s %%s\n' % char or '%s\n'
        for f in filenames:
            ui.write(format % repo.pathto(f, cwd))

def shrink(ui, repo, *pats, **opts):
    '''revert expanded keywords in working directory

    Run before changing/disabling active keywords
    or if you experience problems with "hg import" or "hg merge".

    kwshrink refuses to run if given files contain local changes.
    '''
    # 3rd argument sets expansion to False
    _kwfwrite(ui, repo, False, *pats, **opts)


def reposetup(ui, repo):
    '''Sets up repo as kwrepo for keyword substitution.
    Overrides file method to return kwfilelog instead of filelog
    if file matches user configuration.
    Wraps commit to overwrite configured files with updated
    keyword substitutions.
    This is done for local repos only, and only if there are
    files configured at all for keyword substitution.'''

    try:
        if (not repo.local() or kwtools['hgcmd'] in nokwcommands.split()
            or '.hg' in util.splitpath(repo.root)
            or repo._url.startswith('bundle:')):
            return
    except AttributeError:
        pass

    inc, exc = [], ['.hg*']
    for pat, opt in ui.configitems('keyword'):
        if opt != 'ignore':
            inc.append(pat)
        else:
            exc.append(pat)
    if not inc:
        return

    kwtools['templater'] = kwt = kwtemplater(ui, repo, inc, exc)

    class kwrepo(repo.__class__):
        def file(self, f):
            if f[0] == '/':
                f = f[1:]
            return kwfilelog(self.sopener, f)

        def wread(self, filename):
            data = super(kwrepo, self).wread(filename)
            return kwt.wread(filename, data)

        def commit(self, files=None, text='', user=None, date=None,
                   match=util.always, force=False, force_editor=False,
                   p1=None, p2=None, extra={}, empty_ok=False):
            wlock = lock = None
            _p1 = _p2 = None
            try:
                wlock = self.wlock()
                lock = self.lock()
                # store and postpone commit hooks
                commithooks = {}
                for name, cmd in ui.configitems('hooks'):
                    if name.split('.', 1)[0] == 'commit':
                        commithooks[name] = cmd
                        ui.setconfig('hooks', name, None)
                if commithooks:
                    # store parents for commit hook environment
                    if p1 is None:
                        _p1, _p2 = repo.dirstate.parents()
                    else:
                        _p1, _p2 = p1, p2 or nullid
                    _p1 = hex(_p1)
                    if _p2 == nullid:
                        _p2 = ''
                    else:
                        _p2 = hex(_p2)

                node = super(kwrepo,
                             self).commit(files=files, text=text, user=user,
                                          date=date, match=match, force=force,
                                          force_editor=force_editor,
                                          p1=p1, p2=p2, extra=extra,
                                          empty_ok=empty_ok)

                # restore commit hooks
                for name, cmd in commithooks.iteritems():
                    ui.setconfig('hooks', name, cmd)
                if node is not None:
                    kwt.overwrite(node=node)
                    repo.hook('commit', node=node, parent1=_p1, parent2=_p2)
                return node
            finally:
                del wlock, lock

    repo.__class__ = kwrepo
    patch.patchfile.__init__ = _kwpatchfile_init
    patch.diff = _kw_diff
    webcommands.changeset = webcommands.rev = _kwweb_changeset
    webcommands.filediff = webcommands.diff = _kwweb_filediff


cmdtable = {
    'kwdemo':
        (demo,
         [('d', 'default', None, _('show default keyword template maps')),
          ('f', 'rcfile', [], _('read maps from rcfile'))],
         _('hg kwdemo [-d] [-f RCFILE] [TEMPLATEMAP]...')),
    'kwexpand': (expand, commands.walkopts,
                 _('hg kwexpand [OPTION]... [FILE]...')),
    'kwfiles':
        (files,
         [('a', 'all', None, _('show keyword status flags of all files')),
          ('i', 'ignore', None, _('show files excluded from expansion')),
          ('u', 'untracked', None, _('additionally show untracked files')),
         ] + commands.walkopts,
         _('hg kwfiles [OPTION]... [FILE]...')),
    'kwshrink': (shrink, commands.walkopts,
                 _('hg kwshrink [OPTION]... [FILE]...')),
}
