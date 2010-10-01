# keyword.py - $Keyword$ expansion for Mercurial
#
# Copyright 2007-2010 Christian Ebert <blacktrash@gmx.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# $Id$
#
# Keyword expansion hack against the grain of a DSCM
#
# There are many good reasons why this is not needed in a distributed
# SCM, still it may be useful in very small projects based on single
# files (like LaTeX packages), that are mostly addressed to an
# audience not running a version control system.
#
# For in-depth discussion refer to
# <http://mercurial.selenic.com/wiki/KeywordPlan>.
#
# Keyword expansion is based on Mercurial's changeset template mappings.
#
# Binary files are not touched.
#
# Files to act upon/ignore are specified in the [keyword] section.
# Customized keyword template mappings in the [keywordmaps] section.
#
# Run "hg help keyword" and "hg kwdemo" to get info on configuration.

'''expand keywords in tracked files

This extension expands RCS/CVS-like or self-customized $Keywords$ in
tracked text files selected by your configuration.

Keywords are only expanded in local repositories and not stored in the
change history. The mechanism can be regarded as a convenience for the
current user or for archive distribution.

Keywords expand to the changeset data pertaining to the latest change
relative to the working directory parent of each file.

Configuration is done in the [keyword], [keywordset] and [keywordmaps]
sections of hgrc files.

Example::

    [keyword]
    # expand keywords in every python file except those matching "x*"
    **.py =
    x*    = ignore

    [keywordset]
    # prefer svn- over cvs-like default keywordmaps
    svn = True

.. note::
   The more specific you are in your filename patterns the less you
   lose speed in huge repositories.

For [keywordmaps] template mapping and expansion demonstration and
control run :hg:`kwdemo`. See :hg:`help templates` for a list of
available templates and filters.

Three additional date template filters are provided::

    utcdate      "2006/09/18 15:13:13"
    svnutcdate   "2006-09-18 15:13:13Z"
    svnisodate   "2006-09-18 08:13:13 -700 (Mon, 18 Sep 2006)"

The default template mappings (view with :hg:`kwdemo -d`) can be
replaced with customized keywords and templates. Again, run
:hg:`kwdemo` to control the results of your config changes.

Before changing/disabling active keywords, run :hg:`kwshrink` to avoid
the risk of inadvertently storing expanded keywords in the change
history.

To force expansion after enabling it, or a configuration change, run
:hg:`kwexpand`.

Expansions spanning more than one line and incremental expansions,
like CVS' $Log$, are not supported. A keyword template map "Log =
{desc}" expands to the first line of the changeset description.
'''

from mercurial import commands, cmdutil, dispatch, filelog, revlog, extensions
from mercurial import patch, localrepo, templater, templatefilters, util, match
from mercurial.hgweb import webcommands
from mercurial.i18n import _
import re, shutil, tempfile

commands.optionalrepo += ' kwdemo'

# hg commands that do not act on keywords
nokwcommands = ('add addremove annotate bundle copy export grep incoming init'
                ' log outgoing push rename tip verify convert email glog')

# hg commands that trigger expansion only when writing to working dir,
# not when reading filelog, and unexpand when reading from working dir
restricted = 'merge record qrecord resolve transplant'

# commands using dorecord
recordcommands = 'record qrecord'
# names of extensions using dorecord
recordextensions = 'record'

# date like in cvs' $Date
utcdate = lambda x: util.datestr((x[0], 0), '%Y/%m/%d %H:%M:%S')
# date like in svn's $Date
svnisodate = lambda x: util.datestr(x, '%Y-%m-%d %H:%M:%S %1%2 (%a, %d %b %Y)')
# date like in svn's $Id
svnutcdate = lambda x: util.datestr((x[0], 0), '%Y-%m-%d %H:%M:%SZ')

# make keyword tools accessible
kwtools = {'templater': None, 'hgcmd': ''}


def _defaultkwmaps(ui):
    '''Returns default keywordmaps according to keywordset configuration.'''
    templates = {
        'Revision': '{node|short}',
        'Author': '{author|user}',
    }
    kwsets = ({
        'Date': '{date|utcdate}',
        'RCSfile': '{file|basename},v',
        'RCSFile': '{file|basename},v', # kept for backwards compatibility
                                        # with hg-keyword
        'Source': '{root}/{file},v',
        'Id': '{file|basename},v {node|short} {date|utcdate} {author|user}',
        'Header': '{root}/{file},v {node|short} {date|utcdate} {author|user}',
    }, {
        'Date': '{date|svnisodate}',
        'Id': '{file|basename},v {node|short} {date|svnutcdate} {author|user}',
        'LastChangedRevision': '{node|short}',
        'LastChangedBy': '{author|user}',
        'LastChangedDate': '{date|svnisodate}',
    })
    templates.update(kwsets[ui.configbool('keywordset', 'svn')])
    return templates

class kwtemplater(object):
    '''
    Sets up keyword templates, corresponding keyword regex, and
    provides keyword substitution functions.
    '''

    def __init__(self, ui, repo, inc, exc):
        self.ui = ui
        self.repo = repo
        self.match = match.match(repo.root, '', [], inc, exc)
        self.restrict = kwtools['hgcmd'] in restricted.split()
        self.record = kwtools['hgcmd'] in recordcommands.split()

        kwmaps = self.ui.configitems('keywordmaps')
        if kwmaps: # override default templates
            self.templates = dict((k, templater.parsestring(v, False))
                                  for k, v in kwmaps)
        else:
            self.templates = _defaultkwmaps(self.ui)
        escaped = map(re.escape, self.templates.keys())
        kwpat = r'\$(%s)(: [^$\n\r]*? )??\$' % '|'.join(escaped)
        self.re_kw = re.compile(kwpat)

        templatefilters.filters.update({'utcdate': utcdate,
                                        'svnisodate': svnisodate,
                                        'svnutcdate': svnutcdate})

    def substitute(self, data, path, ctx, subfunc):
        '''Replaces keywords in data with expanded template.'''
        def kwsub(mobj):
            kw = mobj.group(1)
            ct = cmdutil.changeset_templater(self.ui, self.repo,
                                             False, None, '', False)
            ct.use_template(self.templates[kw])
            self.ui.pushbuffer()
            ct.show(ctx, root=self.repo.root, file=path)
            ekw = templatefilters.firstline(self.ui.popbuffer())
            return '$%s: %s $' % (kw, ekw)
        return subfunc(kwsub, data)

    def expand(self, path, node, data):
        '''Returns data with keywords expanded.'''
        if not self.restrict and self.match(path) and not util.binary(data):
            ctx = self.repo.filectx(path, fileid=node).changectx()
            return self.substitute(data, path, ctx, self.re_kw.sub)
        return data

    def iskwfile(self, path, flagfunc):
        '''Returns true if path matches [keyword] pattern
        and is not a symbolic link.
        Caveat: localrepository._link fails on Windows.'''
        return self.match(path) and not 'l' in flagfunc(path)

    def overwrite(self, ctx, candidates, iswctx, expand, changed):
        '''Overwrites selected files expanding/shrinking keywords.'''
        if changed is not None:
            candidates = [f for f in candidates if f in changed]
        candidates = [f for f in candidates if self.iskwfile(f, ctx.flags)]
        if candidates:
            restrict = self.restrict
            self.restrict = True        # do not expand when reading
            rollback = kwtools['hgcmd'] == 'rollback'
            mf = ctx.manifest()
            msg = (expand and _('overwriting %s expanding keywords\n')
                   or _('overwriting %s shrinking keywords\n'))
            for f in candidates:
                if not self.record and not rollback:
                    data = self.repo.file(f).read(mf[f])
                else:
                    data = self.repo.wread(f)
                if util.binary(data):
                    continue
                if expand:
                    if iswctx:
                        ctx = self.repo.filectx(f, fileid=mf[f]).changectx()
                    data, found = self.substitute(data, f, ctx,
                                                  self.re_kw.subn)
                else:
                    found = self.re_kw.search(data)
                if found:
                    self.ui.note(msg % f)
                    self.repo.wwrite(f, data, mf.flags(f))
                    if iswctx and not rollback:
                        self.repo.dirstate.normal(f)
                    elif self.record:
                        self.repo.dirstate.normallookup(f)
            self.restrict = restrict

    def shrinktext(self, text):
        '''Unconditionally removes all keyword substitutions from text.'''
        return self.re_kw.sub(r'$\1$', text)

    def shrink(self, fname, text):
        '''Returns text with all keyword substitutions removed.'''
        if self.match(fname) and not util.binary(text):
            return self.shrinktext(text)
        return text

    def shrinklines(self, fname, lines):
        '''Returns lines with keyword substitutions removed.'''
        if self.match(fname):
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
    def __init__(self, opener, kwt, path):
        super(kwfilelog, self).__init__(opener, path)
        self.kwt = kwt
        self.path = path

    def read(self, node):
        '''Expands keywords when reading filelog.'''
        data = super(kwfilelog, self).read(node)
        return self.kwt.expand(self.path, node, data)

    def add(self, text, meta, tr, link, p1=None, p2=None):
        '''Removes keyword substitutions when adding to filelog.'''
        text = self.kwt.shrink(self.path, text)
        return super(kwfilelog, self).add(text, meta, tr, link, p1, p2)

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
        return repo.status(match=cmdutil.match(repo, pats, opts), clean=True,
                           unknown=opts.get('unknown') or opts.get('all'))
    if ui.configitems('keyword'):
        raise util.Abort(_('[keyword] patterns cannot match'))
    raise util.Abort(_('no [keyword] patterns configured'))

def _kwfwrite(ui, repo, expand, *pats, **opts):
    '''Selects files and passes them to kwtemplater.overwrite.'''
    wctx = repo[None]
    if len(wctx.parents()) > 1:
        raise util.Abort(_('outstanding uncommitted merge'))
    kwt = kwtools['templater']
    wlock = repo.wlock()
    try:
        status = _status(ui, repo, kwt, *pats, **opts)
        modified, added, removed, deleted, unknown, ignored, clean = status
        if modified or added or removed or deleted:
            raise util.Abort(_('outstanding uncommitted changes'))
        kwt.overwrite(wctx, clean, True, expand, None)
    finally:
        wlock.release()

def demo(ui, repo, *args, **opts):
    '''print [keywordmaps] configuration and an expansion example

    Show current, custom, or default keyword template maps and their
    expansions.

    Extend the current configuration by specifying maps as arguments
    and using -f/--rcfile to source an external hgrc file.

    Use -d/--default to disable current configuration.

    See :hg:`help templates` for information on templates and filters.
    '''
    def demoitems(section, items):
        ui.write('[%s]\n' % section)
        for k, v in sorted(items):
            ui.write('%s = %s\n' % (k, v))

    fn = 'demo.txt'
    tmpdir = tempfile.mkdtemp('', 'kwdemo.')
    ui.note(_('creating temporary repository at %s\n') % tmpdir)
    repo = localrepo.localrepository(ui, tmpdir, True)
    ui.setconfig('keyword', fn, '')

    uikwmaps = ui.configitems('keywordmaps')
    if args or opts.get('rcfile'):
        ui.status(_('\n\tconfiguration using custom keyword template maps\n'))
        if uikwmaps:
            ui.status(_('\textending current template maps\n'))
        if opts.get('default') or not uikwmaps:
            ui.status(_('\toverriding default template maps\n'))
        if opts.get('rcfile'):
            ui.readconfig(opts.get('rcfile'))
        if args:
            # simulate hgrc parsing
            rcmaps = ['[keywordmaps]\n'] + [a + '\n' for a in args]
            fp = repo.opener('hgrc', 'w')
            fp.writelines(rcmaps)
            fp.close()
            ui.readconfig(repo.join('hgrc'))
        kwmaps = dict(ui.configitems('keywordmaps'))
    elif opts.get('default'):
        ui.status(_('\n\tconfiguration using default keyword template maps\n'))
        kwmaps = _defaultkwmaps(ui)
        if uikwmaps:
            ui.status(_('\tdisabling current template maps\n'))
            for k, v in kwmaps.iteritems():
                ui.setconfig('keywordmaps', k, v)
    else:
        ui.status(_('\n\tconfiguration using current keyword template maps\n'))
        kwmaps = dict(uikwmaps) or _defaultkwmaps(ui)

    uisetup(ui)
    reposetup(ui, repo)
    ui.write('[extensions]\nkeyword =\n')
    demoitems('keyword', ui.configitems('keyword'))
    demoitems('keywordmaps', kwmaps.iteritems())
    keywords = '$' + '$\n$'.join(sorted(kwmaps.keys())) + '$\n'
    repo.wopener(fn, 'w').write(keywords)
    repo[None].add([fn])
    ui.note(_('\nkeywords written to %s:\n') % fn)
    ui.note(keywords)
    repo.dirstate.setbranch('demobranch')
    for name, cmd in ui.configitems('hooks'):
        if name.split('.', 1)[0].find('commit') > -1:
            repo.ui.setconfig('hooks', name, '')
    msg = _('hg keyword configuration and expansion example')
    ui.note("hg ci -m '%s'\n" % msg)
    repo.commit(text=msg)
    ui.status(_('\n\tkeywords expanded\n'))
    ui.write(repo.wread(fn))
    shutil.rmtree(tmpdir, ignore_errors=True)

def expand(ui, repo, *pats, **opts):
    '''expand keywords in the working directory

    Run after (re)enabling keyword expansion.

    kwexpand refuses to run if given files contain local changes.
    '''
    # 3rd argument sets expansion to True
    _kwfwrite(ui, repo, True, *pats, **opts)

def files(ui, repo, *pats, **opts):
    '''show files configured for keyword expansion

    List which files in the working directory are matched by the
    [keyword] configuration patterns.

    Useful to prevent inadvertent keyword expansion and to speed up
    execution by including only files that are actual candidates for
    expansion.

    See :hg:`help keyword` on how to construct patterns both for
    inclusion and exclusion of files.

    With -A/--all and -v/--verbose the codes used to show the status
    of files are::

      K = keyword expansion candidate
      k = keyword expansion candidate (not tracked)
      I = ignored
      i = ignored (not tracked)
    '''
    kwt = kwtools['templater']
    status = _status(ui, repo, kwt, *pats, **opts)
    cwd = pats and repo.getcwd() or ''
    modified, added, removed, deleted, unknown, ignored, clean = status
    files = []
    if not opts.get('unknown') or opts.get('all'):
        files = sorted(modified + added + clean)
    wctx = repo[None]
    kwfiles = [f for f in files if kwt.iskwfile(f, wctx.flags)]
    kwunknown = [f for f in unknown if kwt.iskwfile(f, wctx.flags)]
    if not opts.get('ignore') or opts.get('all'):
        showfiles = kwfiles, kwunknown
    else:
        showfiles = [], []
    if opts.get('all') or opts.get('ignore'):
        showfiles += ([f for f in files if f not in kwfiles],
                      [f for f in unknown if f not in kwunknown])
    for char, filenames in zip('KkIi', showfiles):
        fmt = (opts.get('all') or ui.verbose) and '%s %%s\n' % char or '%s\n'
        for f in filenames:
            ui.write(fmt % repo.pathto(f, cwd))

def shrink(ui, repo, *pats, **opts):
    '''revert expanded keywords in the working directory

    Run before changing/disabling active keywords or if you experience
    problems with :hg:`import` or :hg:`merge`.

    kwshrink refuses to run if given files contain local changes.
    '''
    # 3rd argument sets expansion to False
    _kwfwrite(ui, repo, False, *pats, **opts)


def uisetup(ui):
    ''' Monkeypatches dispatch._parse to retrieve user command.'''

    def kwdispatch_parse(orig, ui, args):
        '''Monkeypatch dispatch._parse to obtain running hg command.'''
        cmd, func, args, options, cmdoptions = orig(ui, args)
        kwtools['hgcmd'] = cmd
        return cmd, func, args, options, cmdoptions

    extensions.wrapfunction(dispatch, '_parse', kwdispatch_parse)

def reposetup(ui, repo):
    '''Sets up repo as kwrepo for keyword substitution.
    Overrides file method to return kwfilelog instead of filelog
    if file matches user configuration.
    Wraps commit to overwrite configured files with updated
    keyword substitutions.
    Monkeypatches patch and webcommands.'''

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
            return kwfilelog(self.sopener, kwt, f)

        def wread(self, filename):
            data = super(kwrepo, self).wread(filename)
            return kwt.wread(filename, data)

        def commit(self, *args, **opts):
            # use custom commitctx for user commands
            # other extensions can still wrap repo.commitctx directly
            self.commitctx = self.kwcommitctx
            try:
                return super(kwrepo, self).commit(*args, **opts)
            finally:
                del self.commitctx

        def kwcommitctx(self, ctx, error=False):
            n = super(kwrepo, self).commitctx(ctx, error)
            # no lock needed, only called from repo.commit() which already locks
            if not kwt.record:
                kwt.overwrite(self[n], sorted(ctx.added() + ctx.modified()),
                              False, True, None)
            return n

        def rollback(self, dryrun=False):
            wlock = repo.wlock()
            try:
                if not dryrun:
                    changed = self['.'].files()
                ret = super(kwrepo, self).rollback(dryrun)
                if not dryrun:
                    ctx = self['.']
                    modified, added = super(kwrepo, self).status()[:2]
                    kwt.overwrite(ctx, added, True, False, changed)
                    kwt.overwrite(ctx, modified, True, True, changed)
                return ret
            finally:
                wlock.release()

    # monkeypatches
    def kwpatchfile_init(orig, self, ui, fname, opener,
                         missing=False, eolmode=None):
        '''Monkeypatch/wrap patch.patchfile.__init__ to avoid
        rejects or conflicts due to expanded keywords in working dir.'''
        orig(self, ui, fname, opener, missing, eolmode)
        # shrink keywords read from working dir
        self.lines = kwt.shrinklines(self.fname, self.lines)

    def kw_diff(orig, repo, node1=None, node2=None, match=None, changes=None,
                opts=None, prefix=''):
        '''Monkeypatch patch.diff to avoid expansion.'''
        kwt.restrict = True
        return orig(repo, node1, node2, match, changes, opts, prefix)

    def kwweb_skip(orig, web, req, tmpl):
        '''Wraps webcommands.x turning off keyword expansion.'''
        kwt.match = util.never
        return orig(web, req, tmpl)

    def kw_dorecord(orig, ui, repo, commitfunc, *pats, **opts):
        '''Wraps record.dorecord expanding keywords after recording.'''
        wlock = repo.wlock()
        try:
            # record returns 0 even when nothing has changed
            # therefore compare nodes before and after
            ctx = repo['.']
            ret = orig(ui, repo, commitfunc, *pats, **opts)
            recordctx = repo['.']
            if ctx != recordctx:
                kwt.overwrite(recordctx, recordctx.files(),
                              False, True, recordctx)
            return ret
        finally:
            wlock.release()

    repo.__class__ = kwrepo

    extensions.wrapfunction(patch.patchfile, '__init__', kwpatchfile_init)
    extensions.wrapfunction(patch, 'diff', kw_diff)
    for c in 'annotate changeset rev filediff diff'.split():
        extensions.wrapfunction(webcommands, c, kwweb_skip)
    for name in recordextensions.split():
        try:
            record = extensions.find(name)
            extensions.wrapfunction(record, 'dorecord', kw_dorecord)
        except KeyError:
            pass

cmdtable = {
    'kwdemo':
        (demo,
         [('d', 'default', None, _('show default keyword template maps')),
          ('f', 'rcfile', '',
           _('read maps from rcfile'), _('FILE'))],
         _('hg kwdemo [-d] [-f RCFILE] [TEMPLATEMAP]...')),
    'kwexpand': (expand, commands.walkopts,
                 _('hg kwexpand [OPTION]... [FILE]...')),
    'kwfiles':
        (files,
         [('A', 'all', None, _('show keyword status flags of all files')),
          ('i', 'ignore', None, _('show files excluded from expansion')),
          ('u', 'unknown', None, _('only show unknown (not tracked) files')),
         ] + commands.walkopts,
         _('hg kwfiles [OPTION]... [FILE]...')),
    'kwshrink': (shrink, commands.walkopts,
                 _('hg kwshrink [OPTION]... [FILE]...')),
}
