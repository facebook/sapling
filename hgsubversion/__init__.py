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
from mercurial import cmdutil as hgcmdutil

from svn import core

import svncommands
import cmdutil
import svnrepo
import util
import wrappers
import svnexternals

optionmap = {
    'tagpaths': ('hgsubversion', 'tagpaths'),
    'authors': ('hgsubversion', 'authormap'),
    'filemap': ('hgsubversion', 'filemap'),
    'stupid': ('hgsubversion', 'stupid'),
    'defaulthost': ('hgsubversion', 'defaulthost'),
    'defaultauthors': ('hgsubversion', 'defaultauthors'),
    'usebranchnames': ('hgsubversion', 'usebranchnames'),
}
dontretain = { 'hgsubversion': set(['authormap', 'filemap']) }

svnopts = (('', 'stupid', None, 'use slower, but more compatible, protocol for '
            'Subversion'),)

svncloneopts = (('T', 'tagpaths', '', 'list of path s to search for tags '
                 'in Subversion repositories'),
                ('A', 'authors', '', 'path to file mapping Subversion '
                 'usernames to Mercurial authors'),
                ('', 'filemap', '', 'path to file containing rules for '
                 'remapping Subversion repository paths'),)

def wrapper(orig, ui, repo, *args, **opts):
    """
    Subversion %(target)s can be used for %(command)s. See 'hg help
    %(extension)s' for more on the conversion process.
    """
    for opt, (section, name) in optionmap.iteritems():
        if opt in opts and opts[opt]:
            if isinstance(repo, str):
                ui.setconfig(section, name, opts.pop(opt))
            else:
                repo.ui.setconfig(section, name, opts.pop(opt))

    return orig(ui, repo, *args, **opts)

def clonewrapper(orig, ui, source, dest=None, **opts):
    """
    Some of the options listed below only apply to Subversion
    %(target)s. See 'hg help %(extension)s' for more information on
    them as well as other ways of customising the conversion process.
    """

    for opt, (section, name) in optionmap.iteritems():
        if opt in opts and opts[opt]:
            ui.setconfig(section, name, str(opts.pop(opt)))

    # this must be kept in sync with mercurial/commands.py
    srcrepo, dstrepo = hg.clone(hgcmdutil.remoteui(ui, opts), source, dest,
                                pull=opts.get('pull'),
                                stream=opts.get('uncompressed'),
                                rev=opts.get('rev'),
                                update=not opts.get('noupdate'))

    if dstrepo.local() and srcrepo.capable('subversion'):
        fd = dstrepo.opener("hgrc", "a", text=True)
        for section in set(s for s, v in optionmap.itervalues()):
            config = dict(ui.configitems(section))
            for name in dontretain[section]:
                config.pop(name, None)

            if config:
                fd.write('\n[%s]\n' % section)
                map(fd.write, ('%s = %s\n' % p for p in config.iteritems()))

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

    for command, target, isclone in [('clone', 'sources', True),
                                     ('pull', 'sources', False),
                                     ('push', 'destinations', False)]:
        doc = wrapper.__doc__.strip() % { 'command': command,
                                          'Command': command.capitalize(),
                                          'extension': 'hgsubversion',
                                          'target': target }
        fn = getattr(commands, command)
        fn.__doc__ = fn.__doc__.rstrip() + '\n\n    ' + doc
        entry = extensions.wrapcommand(commands.table, command,
                                       (wrapper, clonewrapper)[isclone])
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
