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

from mercurial import commands, cmdutil, context, dispatch, filelog
from mercurial import patch, localrepo, revlog, templater, util
from mercurial.node import *
from mercurial.i18n import _
import re, shutil, sys, tempfile, time

commands.optionalrepo += ' kwdemo'

def utcdate(date):
    '''Returns hgdate in cvs-like UTC format.'''
    return time.strftime('%Y/%m/%d %H:%M:%S', time.gmtime(date[0]))

def _kwrestrict(cmd):
    '''Returns True if cmd should trigger restricted expansion.
    Keywords will only expanded when writing to working dir.
    Crucial for mq as expanded keywords should not make it into patches.'''
    return cmd in ('diff1', 
                   'qimport', 'qnew', 'qpush', 'qrefresh', 'record', 'qrecord')


_kwtemplater = None

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

    def __init__(self, ui, repo, inc, exc, hgcmd):
        self.ui = ui
        self.repo = repo
        self.matcher = util.matcher(repo.root, inc=inc, exc=exc)[1]
        self.hgcmd = hgcmd
        self.commitnode = None
        self.path = ''

        kwmaps = self.ui.configitems('keywordmaps')
        if kwmaps: # override default templates
            kwmaps = [(k, templater.parsestring(v, quoted=False))
                      for (k, v) in kwmaps]
            self.templates = dict(kwmaps)
        escaped = map(re.escape, self.templates.keys())
        kwpat = r'\$(%s)(: [^$\n\r]*? )??\$' % '|'.join(escaped)
        self.re_kw = re.compile(kwpat)

        templater.common_filters['utcdate'] = utcdate
        self.ct = cmdutil.changeset_templater(self.ui, self.repo,
                                              False, '', False)

    def substitute(self, node, data, subfunc):
        '''Obtains file's changenode if commit node not given,
        and calls given substitution function.'''
        if self.commitnode:
            fnode = self.commitnode
        else:
            c = context.filectx(self.repo, self.path, fileid=node)
            fnode = c.node()

        def kwsub(mobj):
            '''Substitutes keyword using corresponding template.'''
            kw = mobj.group(1)
            self.ct.use_template(self.templates[kw])
            self.ui.pushbuffer()
            self.ct.show(changenode=fnode, root=self.repo.root, file=self.path)
            return '$%s: %s $' % (kw, templater.firstline(self.ui.popbuffer()))

        return subfunc(kwsub, data)

    def expand(self, node, data):
        '''Returns data with keywords expanded.'''
        if util.binary(data) or _kwrestrict(self.hgcmd):
            return data
        return self.substitute(node, data, self.re_kw.sub)

    def process(self, node, data, expand):
        '''Returns a tuple: data, count.
        Count is number of keywords/keyword substitutions,
        telling caller whether to act on file containing data.'''
        if util.binary(data):
            return data, None
        if expand:
            return self.substitute(node, data, self.re_kw.subn)
        return data, self.re_kw.search(data)

    def shrink(self, text):
        '''Returns text with all keyword substitutions removed.'''
        if util.binary(text):
            return text
        return self.re_kw.sub(r'$\1$', text)

class kwfilelog(filelog.filelog):
    '''
    Subclass of filelog to hook into its read, add, cmp methods.
    Keywords are "stored" unexpanded, and processed on reading.
    '''
    def __init__(self, opener, path):
        super(kwfilelog, self).__init__(opener, path)
        _kwtemplater.path = path

    def kwctread(self, node, expand):
        '''Reads expanding and counting keywords, called from _overwrite.'''
        data = super(kwfilelog, self).read(node)
        return _kwtemplater.process(node, data, expand)

    def read(self, node):
        '''Expands keywords when reading filelog.'''
        data = super(kwfilelog, self).read(node)
        return _kwtemplater.expand(node, data)

    def add(self, text, meta, tr, link, p1=None, p2=None):
        '''Removes keyword substitutions when adding to filelog.'''
        text = _kwtemplater.shrink(text)
        return super(kwfilelog, self).add(text, meta, tr, link, p1=p1, p2=p2)

    def cmp(self, node, text):
        '''Removes keyword substitutions for comparison.'''
        text = _kwtemplater.shrink(text)
        if self.renamed(node):
            t2 = super(kwfilelog, self).read(node)
            return t2 != text
        return revlog.revlog.cmp(self, node, text)


# store original patch.patchfile.__init__
_patchfile_init = patch.patchfile.__init__

def _kwpatchfile_init(self, ui, fname, missing=False):
    '''Monkeypatch/wrap patch.patchfile.__init__ to avoid
    rejects or conflicts due to expanded keywords in working dir.'''
    _patchfile_init(self, ui, fname, missing=missing)

    if _kwtemplater.matcher(self.fname):
        # shrink keywords read from working dir
        kwshrunk = _kwtemplater.shrink(''.join(self.lines))
        self.lines = kwshrunk.splitlines(True)


def _iskwfile(f, link):
    return not link(f) and _kwtemplater.matcher(f)

def _status(ui, repo, *pats, **opts):
    '''Bails out if [keyword] configuration is not active.
    Returns status of working directory.'''
    if _kwtemplater:
        files, match, anypats = cmdutil.matchpats(repo, pats, opts)
        return repo.status(files=files, match=match, list_clean=True)
    if ui.configitems('keyword'):
        raise util.Abort(_('[keyword] patterns cannot match'))
    raise util.Abort(_('no [keyword] patterns configured'))

def _overwrite(ui, repo, node=None, expand=True, files=None):
    '''Overwrites selected files expanding/shrinking keywords.'''
    ctx = repo.changectx(node)
    mf = ctx.manifest()
    if node is not None:   # commit
        _kwtemplater.commitnode = node
        files = [f for f in ctx.files() if mf.has_key(f)]
        notify = ui.debug
    else:                  # kwexpand/kwshrink
        notify = ui.note
    candidates = [f for f in files if _iskwfile(f, mf.linkf)]
    if candidates:
        candidates.sort()
        action = expand and 'expanding' or 'shrinking'
        for f in candidates:
            fp = repo.file(f, kwmatch=True)
            data, kwfound = fp.kwctread(mf[f], expand)
            if kwfound:
                notify(_('overwriting %s %s keywords\n') % (f, action))
                repo.wwrite(f, data, mf.flags(f))
                repo.dirstate.normal(f)

def _kwfwrite(ui, repo, expand, *pats, **opts):
    '''Selects files and passes them to _overwrite.'''
    status = _status(ui, repo, *pats, **opts)
    modified, added, removed, deleted, unknown, ignored, clean = status
    if modified or added or removed or deleted:
        raise util.Abort(_('outstanding uncommitted changes in given files'))
    wlock = lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        _overwrite(ui, repo, expand=expand, files=clean)
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
            for k, v in kwmaps.items():
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
    demoitems('keywordmaps', kwmaps.items())
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
    status = _status(ui, repo, *pats, **opts)
    modified, added, removed, deleted, unknown, ignored, clean = status
    files = modified + added + clean
    if opts.get('untracked'):
        files += unknown
    files.sort()
    kwfiles = [f for f in files if _iskwfile(f, repo._link)]
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

    if not repo.local():
        return

    nokwcommands = ('add', 'addremove', 'bundle', 'clone', 'copy',
                    'export', 'grep', 'identify', 'incoming', 'init',
                    'log', 'outgoing', 'push', 'remove', 'rename',
                    'rollback', 'tip',
                    'convert')
    hgcmd, func, args, opts, cmdopts = dispatch._parse(ui, sys.argv[1:])
    if hgcmd in nokwcommands:
        return

    if hgcmd == 'diff':
        # only expand if comparing against working dir
        node1, node2 = cmdutil.revpair(repo, cmdopts.get('rev'))
        if node2 is not None:
            return
        # shrink if rev is not current node
        if node1 is not None and node1 != repo.changectx().node():
            hgcmd = 'diff1'

    inc, exc = [], ['.hgtags']
    for pat, opt in ui.configitems('keyword'):
        if opt != 'ignore':
            inc.append(pat)
        else:
            exc.append(pat)
    if not inc:
        return

    global _kwtemplater
    _kwtemplater = kwtemplater(ui, repo, inc, exc, hgcmd)

    class kwrepo(repo.__class__):
        def file(self, f, kwmatch=False):
            if f[0] == '/':
                f = f[1:]
            if kwmatch or _kwtemplater.matcher(f):
                return kwfilelog(self.sopener, f)
            return filelog.filelog(self.sopener, f)

        def wread(self, filename):
            data = super(kwrepo, self).wread(filename)
            if _kwrestrict(hgcmd) and _kwtemplater.matcher(filename):
                return _kwtemplater.shrink(data)
            return data

        def commit(self, files=None, text='', user=None, date=None,
                   match=util.always, force=False, force_editor=False,
                   p1=None, p2=None, extra={}):
            wlock = lock = None
            _p1 = _p2 = None
            try:
                wlock = self.wlock()
                lock = self.lock()
                # store and postpone commit hooks
                commithooks = []
                for name, cmd in ui.configitems('hooks'):
                    if name.split('.', 1)[0] == 'commit':
                        commithooks.append((name, cmd))
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
                                          p1=p1, p2=p2, extra=extra)

                # restore commit hooks
                for name, cmd in commithooks:
                    ui.setconfig('hooks', name, cmd)
                if node is not None:
                    _overwrite(ui, self, node=node)
                    repo.hook('commit', node=node, parent1=_p1, parent2=_p2)
                return node
            finally:
                del wlock, lock

    repo.__class__ = kwrepo
    patch.patchfile.__init__ = _kwpatchfile_init


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
