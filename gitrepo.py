from mercurial import repo, util
from git_handler import GitHandler

class gitrepo(repo.repository):
    capabilities = []
    def __init__(self, ui, path, create):
        if create:
            raise util.Abort('Cannot create a git repository.')
        self.path = path

instance = gitrepo
