import os
import stat
import sys
import traceback

from mercurial import hg
from mercurial import node
from mercurial import util as merc_util

import svnwrap
import util
from util import register_subcommand, svn_subcommands, generate_help
# dirty trick to force demandimport to run my decorator anyway.
from utility_commands import print_wc_url
from fetch_command import fetch_revisions
from push_cmd import commit_from_rev
from diff_cmd import diff_command
from rebuildmeta import rebuildmeta
# shut up, pyflakes, we must import those
__x = [print_wc_url, fetch_revisions, commit_from_rev, diff_command, rebuildmeta]

mode755 = (stat.S_IXUSR | stat.S_IXGRP| stat.S_IXOTH | stat.S_IRUSR |
           stat.S_IRGRP| stat.S_IROTH | stat.S_IWUSR)
mode644 = (stat.S_IRUSR | stat.S_IRGRP| stat.S_IROTH | stat.S_IWUSR)


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
        if subcommand != 'rebuildmeta':
            opts['svn_url'] = open(os.path.join(repo.path, 'svn', 'url')).read()
        return svn_subcommands[subcommand](ui, args=args,
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
    """Get help on the subsubcommands.
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
    """Update to a specified Subversion revision number.
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


def verify_revision(ui, args, repo, force=False, **opts):
    """Verify a single converted revision.
    Note: This wipes your working copy and then exports the corresponding
    Subversion into your working copy to verify. Use with caution.
    """
    assert len(args) == 1
    if not force:
        assert repo.status(ignored=True,
                           unknown=True) == ([], [], [], [], [], [], [])
    rev = int(args[0])
    wc_path = os.path.dirname(repo.path)
    svn_url = open(os.path.join(repo.path, 'svn', 'url')).read()
    svn = svnwrap.SubversionRepo(svn_url, username=merc_util.getuser())
    util.wipe_all_files(wc_path)
    if update(ui, args, repo, clean=True) == 0:
        util.wipe_all_files(wc_path)
        br = repo.dirstate.branch()
        if br == 'default':
            br = None
        if br:
            diff_path = 'branches/%s' % br
        else:
            diff_path = 'trunk'
        svn.fetch_all_files_to_dir(diff_path, rev, wc_path)
        stat = repo.status(unknown=True)
        ignored = [s for s in stat[4]
                   if '/.svn/'  not in s and not s.startswith('.svn/')]
        stat = stat[0:4]
        if stat != ([], [], [], [],) or ignored != []:
            ui.status('Something is wrong with this revision.\n')
            return 2
        else:
            ui.status('OK.\n')
            return 0
    return 1
verify_revision = register_subcommand('verify_revision')(verify_revision)

def verify_all_revisions(ui, args, repo, **opts):
    """Verify all the converted revisions
    optionally starting at a revision.

    Note: This is *extremely* abusive of the Subversion server. It exports every
    revision of the code one revision at a time.
    """
    assert repo.status(ignored=True,
                       unknown=True) == ([], [], [], [], [], [], [])
    start_rev = 0
    args = list(args)
    if args:
        start_rev = int(args.pop(0))
    revmap = util.parse_revmap(os.path.join(repo.path, 'svn', 'rev_map'))
    revs = sorted(revmap.keys())
    for revnum, br in revs:
        if revnum < start_rev:
            continue
        res = verify_revision(ui, [revnum], repo, force=True)
        if res == 0:
            print revnum, 'verfied'
        elif res == 1:
            print revnum, 'skipped'
        else:
            print revnum, 'failed'
            return 1
    return 0
verify_all_revisions = register_subcommand('verify_all_revisions')(verify_all_revisions)
