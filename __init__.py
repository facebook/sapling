import os

from mercurial import commands
from mercurial import hg

import svncommand
import fetch_command
import tag_repo
import util

def reposetup(ui, repo):
    if not util.is_svn_repo(repo):
        return

    repo.__class__ = tag_repo.generate_repo_class(ui, repo)


def svn(ui, repo, subcommand, *args, **opts):
    return svncommand.svncmd(ui, repo, subcommand, *args, **opts)

def svn_fetch(ui, svn_url, hg_repo_path=None, **opts):
    '''Clone Subversion repository to local Mercurial repository.

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
    res = fetch_command.fetch_revisions(ui, svn_url, hg_repo_path, **opts)
    if (res is None or res == 0) and should_update:
        repo = hg.repository(ui, hg_repo_path)
        commands.update(ui, repo, repo['tip'].node())
    return res

commands.norepo += " svnclone"
cmdtable = {
    "svn":
        (svn,
         [('u', 'svn-url', '', 'Path to the Subversion server.'),
          ('', 'stupid', False, 'Be stupid and use diffy replay.'),
          ('A', 'authors', '', 'username mapping filename'),
          ],
         svncommand.generate_help(),
         ),
    "svnclone":
        (svn_fetch,
         [('S', 'skipto-rev', '0', 'Skip commits before this revision.'),
          ('', 'stupid', False, 'Be stupid and use diffy replay.'),
          ('T', 'tag-locations', 'tags', 'Relative path to Subversion tags.'),
          ('A', 'authors', '', 'username mapping filename'),
         ],
         'hg svnclone source [dest]'),
}
