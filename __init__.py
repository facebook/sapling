'''integration with Subversion repositories

This extension allows Mercurial to act as a Subversion client, for
fast incremental, bidirectional updates.

It is *not* ready yet for production use. You should only be using
this if you're ready to hack on it, and go diving into the internals
of Mercurial and/or Subversion.

Before using hgsubversion, it is *strongly* encouraged to run the
automated tests. See `README' in the hgsubversion directory for
details.
'''

import os

from mercurial import commands
from mercurial import hg
from mercurial import util as hgutil

from svn import core

import svncommand
import svncommands
import tag_repo
import util

from util import svn_subcommands, svn_commands_nourl

def reposetup(ui, repo):
    if not util.is_svn_repo(repo):
        return

    repo.__class__ = tag_repo.generate_repo_class(ui, repo)


def svn(ui, repo, subcommand, *args, **opts):
    '''see detailed help for list of subcommands'''

    # guess command if prefix
    if subcommand not in svn_subcommands:
        candidates = []
        for c in svn_subcommands:
            if c.startswith(subcommand):
                candidates.append(c)
        if len(candidates) == 1:
            subcommand = candidates[0]

    path = os.path.dirname(repo.path)
    try:
        commandfunc = svn_subcommands[subcommand]
        if commandfunc not in svn_commands_nourl:
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


def svn_fetch(ui, svn_url, hg_repo_path=None, **opts):
    '''clone Subversion repository to a local Mercurial repository.

    If no destination directory name is specified, it defaults to the
    basename of the source plus "-hg".

    You can specify multiple paths for the location of tags using comma
    separated values.
    '''
    if not hg_repo_path:
        hg_repo_path = hg.defaultdest(svn_url) + "-hg"
        ui.status("Assuming destination %s\n" % hg_repo_path)
    should_update = not os.path.exists(hg_repo_path)
    svn_url = util.normalize_url(svn_url)
    try:
        res = svncommands.pull(ui, svn_url, hg_repo_path, **opts)
    except core.SubversionException, e:
        if e.apr_err == core.SVN_ERR_RA_SERF_SSL_CERT_UNTRUSTED:
            raise hgutil.Abort('It appears svn does not trust the ssl cert for this site.\n'
                     'Please try running svn ls on that url first.')
        raise
    if (res is None or res == 0) and should_update:
        repo = hg.repository(ui, hg_repo_path)
        commands.update(ui, repo, repo['tip'].node())
    return res

commands.norepo += " svnclone"
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
         svncommands.generate_help(),
         ),
    "svnclone":
        (svn_fetch,
         [('S', 'skipto-rev', '0', 'skip commits before this revision.'),
          ('', 'stupid', False, 'be stupid and use diffy replay.'),
          ('T', 'tag-locations', 'tags', 'Relative path to Subversion tags.'),
          ('A', 'authors', '', 'username mapping filename'),
          ('', 'filemap', '',
           'remap file to exclude paths or include only certain paths'),
          ('', 'username', '', 'username for authentication'),
          ('', 'password', '', 'password for authentication'),
         ],
         'hg svnclone source [dest]'),
}
