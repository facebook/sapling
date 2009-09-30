from mercurial import repo, util
from git_handler import GitHandler

class gitrepo(repo.repository):
    capabilities = ['lookup']
    def __init__(self, ui, path, create):
        if create: # pragma: no cover
            raise util.Abort('Cannot create a git repository.')
        self.path = path
    def lookup(self, key):
        if isinstance(key, str):
            return key

instance = gitrepo
