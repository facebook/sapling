"""Layout that allows you to define arbitrary subversion to mercurial mappings.

This is the simplest layout to use if your layout is just plain weird.
Also useful if your layout is pretty normal, but you personally only
want a couple of branches.


"""

import base


class CustomLayout(base.BaseLayout):

    def __init__(self, meta):
        base.BaseLayout.__init__(self, meta)

        self.svn_to_hg = {}
        self.hg_to_svn = {}

        meta._gen_cachedconfig('custombranches', {}, configname='hgsubversionbranch')

        for hg_branch, svn_path in meta.custombranches.iteritems():

            hg_branch = hg_branch.strip()
            if hg_branch == 'default' or not hg_branch:
                hg_branch = None
            svn_path = svn_path.strip('/')

            for other_svn in self.svn_to_hg:
                if other_svn == svn_path:
                    msg = 'specified two hg branches for svn path %s: %s and %s'
                    raise hgutil.Abort(msg % (svn_path, other_hg, hg_branch))

                if (other_svn.startswith(svn_path + '/') or
                    svn_path.startswith(other_svn + '/')):
                    msg = 'specified mappings for nested svn paths: %s and %s'
                    raise hgutl.Abort(msg % (svn_path, other_svn))

            self.svn_to_hg[svn_path] = hg_branch
            self.hg_to_svn[hg_branch] = svn_path

    @property
    def name(self):
        return 'custom'

    def localname(self, path):
        if path in self.svn_to_hg:
            return self.svn_to_hg[path]
        children = []
        for svn_path in self.svn_to_hg:
            if svn_path.startswith(path + '/'):
                children.append(svn_path)
        if len(children) == 1:
            return self.svn_to_hg[children[0]]

        return '../%s' % path

    def remotename(self, branch):
        if branch =='default':
            branch = None
        if branch and branch.startswith('../'):
            return branch[3:]
        if branch not in self.hg_to_svn:
            raise KeyError('Unknown mercurial branch: %s' % branch)
        return self.hg_to_svn[branch]

    def remotepath(self, branch, subdir='/'):
        if not subdir.endswith('/'):
            subdir += '/'
        return subdir + self.remotename(branch)

    @property
    def taglocations(self):
        return []

    def get_path_tag(self, path, taglocations):
        return None

    def split_remote_name(self, path, known_branches):
        if path in self.svn_to_hg:
            return path, ''
        children = []
        for svn_path in self.svn_to_hg:
            if path.startswith(svn_path + '/'):
                return svn_path, path[len(svn_path)+1:]
            if svn_path.startswith(path + '/'):
                children.append(svn_path)

        # if the path represents the parent of exactly one of our svn
        # branches, treat it as though it were that branch, because
        # that means we are probably pulling in a subproject of an svn
        # project, and someone copied the parent svn project.
        if len(children) == 1:
            return children[0], ''

        for branch in known_branches:
            if branch and branch.startswith('../'):
                if path.startswith(branch[3:] + '/'):
                    # -3 for the leading ../, plus one for the trailing /
                    return branch[3:], path[len(branch) - 2:]
                if branch[3:].startswith(path + '/'):
                    children.append(branch[3:])

        if len(children) == 1:
            return children[0], ''


        # this splits on the rightmost '/' but considers the entire
        # string to be the branch component of the path if there is no '/'
        components = path.rsplit('/', 1)
        return components[0], '/'.join(components[1:])
