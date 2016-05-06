'''integration with Subversion repositories

hgsubversion is an extension for Mercurial that allows it to act as a Subversion
client, offering fast, incremental and bidirectional synchronisation.

At this point, hgsubversion is usable by users reasonably familiar with
Mercurial as a VCS. It's not recommended to dive into hgsubversion as an
introduction to Mercurial, since hgsubversion "bends the rules" a little
and violates some of the typical assumptions of early Mercurial users.

Before using hgsubversion, we *strongly* encourage running the
automated tests. See 'README' in the hgsubversion directory for
details.

For more information and instructions, see :hg:`help subversion`.
'''

testedwith = '2.8.2 3.0.1 3.1 3.2.2 3.3 3.4 3.5 3.6 3.7'

import os
import sys
import traceback

from mercurial import commands
try:
    from mercurial import exchange
    exchange.push  # existed in first iteration of this file
except ImportError:
    # We only *use* the exchange module in hg 3.2+, so this is safe
    pass
from mercurial import extensions
from mercurial import help
from mercurial import hg
from mercurial import localrepo
from mercurial import util as hgutil
from mercurial import demandimport
demandimport.ignore.extend([
    'svn',
    'svn.client',
    'svn.core',
    'svn.delta',
    'svn.ra',
    ])

from mercurial import templatekw
from mercurial import revset
from mercurial import subrepo

import svncommands
import util
import svnrepo
import wrappers
import svnexternals

svnopts = [
    ('', 'stupid', None,
     'use slower, but more compatible, protocol for Subversion'),
]

# generic means it picks up all options from svnopts
# fixdoc means update the docstring
# TODO: fixdoc hoses l18n
wrapcmds = { # cmd: generic, target, fixdoc, ppopts, opts
    'parents': (False, None, False, False, [
        ('', 'svn', None, 'show parent svn revision instead'),
    ]),
    'diff': (False, None, False, False, [
        ('', 'svn', None, 'show svn diffs against svn parent'),
    ]),
    'pull': (True, 'sources', True, True, []),
    'push': (True, 'destinations', True, True, []),
    'incoming': (False, 'sources', True, True, []),
    'version': (False, None, False, False, [
        ('', 'svn', None, 'print hgsubversion information as well')]),
    'clone': (False, 'sources', True, True, [
        ('T', 'tagpaths', '',
         'list of paths to search for tags in Subversion repositories'),
        ('', 'branchdir', '',
         'path to search for branches in subversion repositories'),
        ('', 'trunkdir', '',
         'path to trunk in subversion repositories'),
        ('', 'infix', '',
         'path relative to trunk, branch an tag dirs to import'),
        ('A', 'authors', '',
         'file mapping Subversion usernames to Mercurial authors'),
        ('', 'filemap', '',
         'file containing rules for remapping Subversion repository paths'),
        ('', 'layout', 'auto', ('import standard layout or single '
                                'directory? Can be standard, single, or auto.')),
        ('', 'branchmap', '', 'file containing rules for branch conversion'),
        ('', 'tagmap', '', 'file containing rules for renaming tags'),
        ('', 'startrev', '', ('convert Subversion revisions starting at the one '
                              'specified, either an integer revision or HEAD; '
                              'HEAD causes only the latest revision to be '
                              'pulled')),
    ]),
}

try:
    from mercurial import discovery
    def findcommonoutgoing(orig, *args, **opts):
        capable = getattr(args[1], 'capable', lambda x: False)
        if capable('subversion'):
            return wrappers.findcommonoutgoing(*args, **opts)
        else:
            return orig(*args, **opts)
    extensions.wrapfunction(discovery, 'findcommonoutgoing', findcommonoutgoing)
except AttributeError:
    # only need the discovery variant of this code when we drop hg < 1.6
    def findoutgoing(orig, *args, **opts):
        capable = getattr(args[1], 'capable', lambda x: False)
        if capable('subversion'):
            return wrappers.findoutgoing(*args, **opts)
        else:
            return orig(*args, **opts)
    extensions.wrapfunction(discovery, 'findoutgoing', findoutgoing)
except ImportError:
    pass

def extsetup(ui):
    """insert command wrappers for a bunch of commands"""

    docvals = {'extension': 'hgsubversion'}
    for cmd, (generic, target, fixdoc, ppopts, opts) in wrapcmds.iteritems():

        if fixdoc and wrappers.generic.__doc__:
            docvals['command'] = cmd
            docvals['Command'] = cmd.capitalize()
            docvals['target'] = target
            doc = wrappers.generic.__doc__.strip() % docvals
            fn = getattr(commands, cmd)
            fn.__doc__ = fn.__doc__.rstrip() + '\n\n    ' + doc

        wrapped = generic and wrappers.generic or getattr(wrappers, cmd)
        entry = extensions.wrapcommand(commands.table, cmd, wrapped)
        if ppopts:
            entry[1].extend(svnopts)
        if opts:
            entry[1].extend(opts)

    try:
        rebase = extensions.find('rebase')
        if not rebase:
            return
        entry = extensions.wrapcommand(rebase.cmdtable, 'rebase', wrappers.rebase)
        entry[1].append(('', 'svn', None, 'automatic svn rebase'))
    except:
        pass

    if not hgutil.safehasattr(localrepo.localrepository, 'push'):
        # Mercurial >= 3.2
        extensions.wrapfunction(exchange, 'push', wrappers.exchangepush)
    if not hgutil.safehasattr(localrepo.localrepository, 'pull'):
        # Mercurial >= 3.2
        extensions.wrapfunction(exchange, 'pull', wrappers.exchangepull)

    helpdir = os.path.join(os.path.dirname(__file__), 'help')

    entries = (
        (['subversion'],
         "Working with Subversion Repositories",
         # Mercurial >= 3.6: doc(ui)
         lambda *args: open(os.path.join(helpdir, 'subversion.rst')).read()),
    )

    help.helptable.extend(entries)

    templatekeywords = {
        'svnrev': svnrevkw,
        'svnpath': svnpathkw,
        'svnuuid': svnuuidkw,
    }

    templatekw.keywords.update(templatekeywords)

    revset.symbols.update(util.revsets)

    subrepo.types['hgsubversion'] = svnexternals.svnsubrepo

def reposetup(ui, repo):
    if repo.local():
        svnrepo.generate_repo_class(ui, repo)
        for tunnel in ui.configlist('hgsubversion', 'tunnels'):
            hg.schemes['svn+' + tunnel] = svnrepo

    if ui.configbool('hgsubversion', 'nativerevs'):
        extensions.wrapfunction(revset, 'stringset', util.revset_stringset)
        revset.symbols['stringset'] = revset.stringset
        revset.methods['string'] = revset.stringset
        revset.methods['symbol'] = revset.stringset

_old_local = hg.schemes['file']
def _lookup(url):
    if util.islocalrepo(url):
        return svnrepo
    else:
        return _old_local(url)

# install scheme handlers
hg.schemes.update({ 'file': _lookup, 'http': svnrepo, 'https': svnrepo,
                    'svn': svnrepo, 'svn+ssh': svnrepo, 'svn+http': svnrepo,
                    'svn+https': svnrepo})

if hgutil.safehasattr(commands, 'optionalrepo'):
    commands.optionalrepo += ' svn'

cmdtable = {
    "svn":
        (svncommands.svn,
         [('u', 'svn-url', '', 'path to the Subversion server.'),
          ('', 'stupid', False, 'be stupid and use diffy replay.'),
          ('A', 'authors', '', 'username mapping filename'),
          ('', 'filemap', '',
           'remap file to exclude paths or include only certain paths'),
          ('', 'force', False, 'force an operation to happen'),
          ('', 'username', '', 'username for authentication'),
          ('', 'password', '', 'password for authentication'),
          ('r', 'rev', '', 'Mercurial revision'),
          ('', 'unsafe-skip-uuid-check', False,
           'skip repository uuid check in rebuildmeta'),
          ],
         'hg svn <subcommand> ...',
         ),
}

# only these methods are public
__all__ = ('cmdtable', 'reposetup', 'uisetup')

def _templatehelper(ctx, kw):
    '''
    Helper function for displaying information about converted changesets.
    '''
    convertinfo = util.getsvnrev(ctx, '')

    if not convertinfo or not convertinfo.startswith('svn:'):
        return ''

    if kw == 'svnuuid':
        return convertinfo[4:40]
    elif kw == 'svnpath':
        return convertinfo[40:].rsplit('@', 1)[0]
    elif kw == 'svnrev':
        return convertinfo[40:].rsplit('@', 1)[-1]
    else:
        raise hgutil.Abort('unrecognized hgsubversion keyword %s' % kw)

def svnrevkw(**args):
    """:svnrev: String. Converted subversion revision number."""
    return _templatehelper(args['ctx'], 'svnrev')

def svnpathkw(**args):
    """:svnpath: String. Converted subversion revision project path."""
    return _templatehelper(args['ctx'], 'svnpath')

def svnuuidkw(**args):
    """:svnuuid: String. Converted subversion revision repository identifier."""
    return _templatehelper(args['ctx'], 'svnuuid')

def listsvnkeys(repo):
    keys = {}
    repo = repo.local()
    metadir = os.path.join(repo.path, 'svn')

    if util.subversionmetaexists(repo.path):
        w = repo.wlock()
        try:
            for key in util.pushkeyfiles:
                fullpath = os.path.join(metadir, key)
                if os.path.isfile(fullpath):
                    data = open(fullpath).read()

                    # Some of the files could be large, but also quite compressible
                    keys[key] = base85.b85encode(zlib.compress(data))
        finally:
            w.release()

    return keys
