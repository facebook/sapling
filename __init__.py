'''integration with Subversion repositories

hgsubversion is an extension for Mercurial that allows it to act as a Subversion
client, offering fast, incremental and bidirectional synchronisation.

Please note that hgsubversion should not be considered stable software. It is
not feature complete, and neither guarantees of functionality nor future
compatability can be offered. It is, however, quite useful for the cases where
it works, and a good platform for further improvements.

Before using hgsubversion, we *strongly* encourage running the
automated tests. See `README' in the hgsubversion directory for
details.

The operation of hgsubversion can be customised with the following variables:

<list not written yet>

'''
# TODO: The docstring should be slightly more helpful, and at least mention all
#       configuration settings we support

import os
import sys
import traceback

from mercurial import commands
from mercurial import extensions
from mercurial import hg
from mercurial import util as hgutil

from svn import core

import svncommands
import svnrepo
import util
import wrappers
import svnexternals

schemes = ('svn', 'svn+ssh', 'svn+http', 'svn+file')

optionmap = {
    'tagpaths': ('hgsubversion', 'tagpaths'),
    'authors': ('hgsubversion', 'authormap'),
    'filemap': ('hgsubversion', 'filemap'),
    'stupid': ('hgsubversion', 'stupid'),
    'defaulthost': ('hgsubversion', 'defaulthost'),
    'defaultauthors': ('hgsubversion', 'defaultauthors'),
    'usebranchnames': ('hgsubversion', 'usebranchnames'),
}

def wrapper(orig, ui, repo, *args, **opts):
    """
    Subversion repositories are also supported for this command. See
    `hg help %(extension)s` for details.
    """
    for opt, (section, name) in optionmap.iteritems():
        if opt in opts:
            if isinstance(repo, str):
                ui.setconfig(section, name, opts.pop(opt))
            else:
                repo.ui.setconfig(section, name, opts.pop(opt))

    return orig(ui, repo, *args, **opts)

def uisetup(ui):
    """Do our UI setup.

    Does the following wrappings:
     * parent -> utility_commands.parent
     * outgoing -> utility_commands.outgoing
     """
    entry = extensions.wrapcommand(commands.table, 'parents',
                                   wrappers.parent)
    entry[1].append(('', 'svn', None, "show parent svn revision instead"))
    entry = extensions.wrapcommand(commands.table, 'outgoing',
                                   wrappers.outgoing)
    entry[1].append(('', 'svn', None, "show revisions outgoing to subversion"))
    entry = extensions.wrapcommand(commands.table, 'diff',
                                   wrappers.diff)
    entry[1].append(('', 'svn', None,
                     "show svn-style diffs, default against svn parent"))

    newflags = (('A', 'authors', '', 'path to file containing username '
                 'mappings for Subversion sources'),
                ('', 'filemap', '', 'path to file containing rules for file '
                 'name mapping used for sources)'),
                ('T', 'tagpaths', ['tags'], 'list of paths to search for tags '
                 'in Subversion repositories.'))
    extname = __package__.split('_')[-1]

    for command in ['clone']:
        doc = wrapper.__doc__.strip() % { 'extension': extname }
        getattr(commands, command).__doc__ += doc
        entry = extensions.wrapcommand(commands.table, command, wrapper)
        entry[1].extend(newflags)
        
    try:
        rebase = extensions.find('rebase')
        if rebase:
            entry = extensions.wrapcommand(rebase.cmdtable, 'rebase', wrappers.rebase)
            entry[1].append(('', 'svn', None, 'automatic svn rebase', ))
    except:
        pass


def svn(ui, repo, subcommand, *args, **opts):
    '''see detailed help for list of subcommands'''

    # guess command if prefix
    if subcommand not in svncommands.table:
        candidates = []
        for c in svncommands.table:
            if c.startswith(subcommand):
                candidates.append(c)
        if len(candidates) == 1:
            subcommand = candidates[0]

    path = os.path.dirname(repo.path)
    try:
        commandfunc = svncommands.table[subcommand]
        if subcommand not in svncommands.nourl:
            opts['svn_url'] = open(os.path.join(repo.path, 'svn', 'url')).read()
        return commandfunc(ui, args=args, hg_repo_path=path, repo=repo, **opts)
    except core.SubversionException, e:
        if e.apr_err == core.SVN_ERR_RA_SERF_SSL_CERT_UNTRUSTED:
            raise hgutil.Abort('It appears svn does not trust the ssl cert for this site.\n'
                     'Please try running svn ls on that url first.')
        raise
    except TypeError:
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) == 1:
            ui.status('Bad arguments for subcommand %s\n' % subcommand)
        else:
            raise
    except KeyError, e:
        tb = traceback.extract_tb(sys.exc_info()[2])
        if len(tb) == 1:
            ui.status('Unknown subcommand %s\n' % subcommand)
        else:
            raise

def reposetup(ui, repo):
    if repo.local():
       svnrepo.generate_repo_class(ui, repo)

for scheme in schemes:
    hg.schemes[scheme] = svnrepo

cmdtable = {
    "svn":
        (svn,
         [('u', 'svn-url', '', 'path to the Subversion server.'),
          ('', 'stupid', False, 'be stupid and use diffy replay.'),
          ('A', 'authors', '', 'username mapping filename'),
          ('', 'filemap', '',
           'remap file to exclude paths or include only certain paths'),
          ('', 'force', False, 'force an operation to happen'),
          ('', 'username', '', 'username for authentication'),
          ('', 'password', '', 'password for authentication'),
          ],
         svncommands._helpgen(),
         ),
}
