from mercurial import hg, repo
from git_handler import GitHandler

class gitrepo(repo.repository):
    capabilities = []

    def __init__(self, ui, path, create=1):
        dest = hg.defaultdest(path)

        if dest.endswith('.git'):
            dest = dest[:-4]

        # create the local hg repo on disk
        dest_repo = hg.repository(ui, dest, create=True)

        # fetch the initial git data
        git = GitHandler(dest_repo, ui)
        git.remote_add('origin', path)
        git.fetch('origin')

        # checkout the tip
        node = git.remote_head('origin')
        hg.update(dest_repo, node)

        raise SystemExit

instance = gitrepo
