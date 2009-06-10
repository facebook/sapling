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
import cmdutil
import svnrepo
import wrappers

svnopts = (('', 'stupid', None, 'use slower, but more compatible, protocol for '
            'Subversion'),)

svncloneopts = (('T', 'tagpaths', '', 'list of path s to search for tags '
                 'in Subversion repositories'),
                ('A', 'authors', '', 'path to file mapping Subversion '
                 'usernames to Mercurial authors'),
                ('', 'filemap', '', 'path to file containing rules for '
                 'remapping Subversion repository paths'),)

wraptype = {False: wrappers.generic, True: wrappers.clone}

def uisetup(ui):
    """Do our UI setup.

    Does the following wrappings:
     * parent -> utility_commands.parent
     * outgoing -> utility_commands.outgoing
     """
    entry = extensions.wrapcommand(commands.table, 'parents',
                                   wrappers.parent)
    entry[1].append(('', 'svn', None, "show parent svn revision instead"))
    entry = extensions.wrapcommand(commands.table, 'diff',
                                   wrappers.diff)
    entry[1].append(('', 'svn', None,
                     "show svn-style diffs, default against svn parent"))

    docvals = {'extension': 'hgsubversion'}
    for command, target, isclone in [('clone', 'sources', True),
                                     ('pull', 'sources', False),
                                     ('push', 'destinations', False)]:

        docvals['command'] = command
        docvals['Command'] = command.capitalize()
        docvals['target'] = target
        doc = wrappers.generic.__doc__.strip() % docvals
        fn = getattr(commands, command)
        fn.__doc__ = fn.__doc__.rstrip() + '\n\n    ' + doc

        wrapped = wraptype[isclone]
        entry = extensions.wrapcommand(commands.table, command, wrapped)
        entry[1].extend(svnopts)
        if isclone: entry[1].extend(svncloneopts)

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


def _lookup(url):
    if cmdutil.islocalrepo(url):
        return svnrepo
    else:
        return hg._local(url)

# install scheme handlers
hg.schemes.update({ 'file': _lookup, 'http': svnrepo, 'https': svnrepo,
                    'svn': svnrepo, 'svn+ssh': svnrepo })

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

# only these methods are public
__all__ = ('cmdtable', 'reposetup', 'uisetup')
