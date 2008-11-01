import os

from mercurial import commands
from mercurial import hg

import svncommand
import fetch_command

def svn(ui, repo, subcommand, *args, **opts):
    return svncommand.svncmd(ui, repo, subcommand, *args, **opts)

def svn_fetch(ui, svn_url, hg_repo_path=None, **opts):
    if not hg_repo_path:
        hg_repo_path = hg.defaultdest(svn_url) + "-hg"
        ui.status("Assuming destination %s\n" % hg_repo_path)
    should_update = not os.path.exists(hg_repo_path)
    res = fetch_command.fetch_revisions(ui, svn_url, hg_repo_path, **opts)
    if (res is None or res == 0) and should_update:
        repo = hg.repository(ui, hg_repo_path)
        commands.update(ui, repo, repo['tip'].node())
    return res

commands.norepo += " svnclone"
cmdtable = {
    "svn":
        (svn,
         [('u', 'svn_url', '', 'Path to the Subversion server.'),
          ('', 'stupid', False, 'Be stupid and use diffy replay.'),
          ],
         'hg svn subcommand'),
    "svnclone" :(svn_fetch,
         [('S', 'skipto_rev', '0', 'Skip commits before this revision.'),
          ('', 'stupid', False, 'Be stupid and use diffy replay.'),
          ('T', 'tag_locations', 'tags', 'Relative path to where tags get '
           'stored, as comma sep. values if there is more than one such path.')
         ],
         'hg svn_fetch svn_url, dest'),
}
