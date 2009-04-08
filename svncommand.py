import os
import stat
import sys
import traceback

from mercurial import hg
from mercurial import node

import svnwrap
import util
from util import register_subcommand, svn_subcommands, generate_help, svn_commands_nourl
# dirty trick to force demandimport to run my decorator anyway.
from svncommands import pull
from utility_commands import print_wc_url
from push_cmd import commit_from_rev
from diff_cmd import diff_command
from rebuildmeta import rebuildmeta
# shut up, pyflakes, we must import those
__x = [print_wc_url, pull, commit_from_rev, diff_command, rebuildmeta]


def svncmd(ui, repo, subcommand, *args, **opts):
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
        return commandfunc(ui, args=args,
                           hg_repo_path=path,
                           repo=repo,
                           **opts)
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


def help_command(ui, args=None, **opts):
    """show help for a given subcommands or a help overview
    """
    if args:
        subcommand = args[0]
        if subcommand not in svn_subcommands:
            candidates = []
            for c in svn_subcommands:
                if c.startswith(subcommand):
                    candidates.append(c)
            if len(candidates) == 1:
                subcommand = candidates[0]
            elif len(candidates) > 1:
                ui.status('Ambiguous command. Could have been:\n%s\n' %
                          ' '.join(candidates))
                return
        doc = svn_subcommands[subcommand].__doc__
        if doc is None:
            doc = "No documentation available for %s." % subcommand
        ui.status(doc.strip(), '\n')
        return
    ui.status(generate_help())
help_command = register_subcommand('help')(help_command)

def update(ui, args, repo, clean=False, **opts):
    """update to a specified Subversion revision number
    """
    assert len(args) == 1
    rev = int(args[0])
    path = os.path.join(repo.path, 'svn', 'rev_map')
    answers = []
    for k,v in util.parse_revmap(path).iteritems():
        if k[0] == rev:
            answers.append((v, k[1]))
    if len(answers) == 1:
        if clean:
            return hg.clean(repo, answers[0][0])
        return hg.update(repo, answers[0][0])
    elif len(answers) == 0:
        ui.status('Revision %s did not produce an hg revision.\n' % rev)
        return 1
    else:
        ui.status('Ambiguous revision!\n')
        ui.status('\n'.join(['%s on %s' % (node.hex(a[0]), a[1]) for a in
                             answers]+['']))
    return 1
update = register_subcommand('up')(update)
