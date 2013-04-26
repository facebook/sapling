

import base


class StandardLayout(base.BaseLayout):
    """The standard trunk, branches, tags layout"""

    def localname(self, path):
        if path == 'trunk':
            return None
        elif path.startswith('branches/'):
            return path[len('branches/'):]
        return  '../%s' % path

    def remotename(self, branch):
        if branch == 'default' or branch is None:
            return 'trunk'
        elif branch.startswith('../'):
            return branch[3:]
        return 'branches/%s' % branch

    def remotepath(self, branch, subdir='/'):
        branchpath = 'trunk'
        if branch:
            if branch.startswith('../'):
                branchpath = branch[3:]
            else:
                branchpath = 'branches/%s' % branch

        return '%s/%s' % (subdir or '', branchpath)
