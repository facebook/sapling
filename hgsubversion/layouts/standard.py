import os.path

import base

class StandardLayout(base.BaseLayout):
    """The standard trunk, branches, tags layout"""

    def __init__(self, meta):
        base.BaseLayout.__init__(self, meta)

        self._tag_locations = None

        # branchdir is expected to be stripped of leading slashes but retain
        # its last slash
        meta._gen_cachedconfig('branchdir', 'branches',
                               pre=lambda x: '/'.join(p for p in x.split('/')
                                                      if p) + '/')

        self._infix = self.meta.ui.config('hgsubversion', 'infix', '').strip('/')
        if self._infix:
            self._infix = '/' + self._infix

    @property
    def trunk(self):
        return 'trunk' + self._infix

    def localname(self, path):
        if path == self.trunk:
            return None
        elif path.startswith(self.meta.branchdir) and path.endswith(self._infix):
            path = path[len(self.meta.branchdir):]
            if self._infix:
                path = path[:-len(self._infix)]
            return path
        return  '../%s' % path

    def remotename(self, branch):
        if branch == 'default' or branch is None:
            path = self.trunk
        elif branch.startswith('../'):
            path =  branch[3:]
        else:
            path = ''.join((self.meta.branchdir, branch, self._infix))

        return path

    def remotepath(self, branch, subdir='/'):
        if subdir == '/':
            subdir = ''
        branchpath = self.trunk
        if branch and branch != 'default':
            if branch.startswith('../'):
                branchpath = branch[3:]
            else:
                branchpath = ''.join((self.meta.branchdir, branch, self._infix))

        return '%s/%s' % (subdir or '', branchpath)

    def taglocations(self, metapath):
        # import late to avoid trouble when running the test suite
        try:
            # newer versions of mercurial >= 2.8 will import this because the
            # hgext_ logic is already being done in core
            from hgsubversion import util
        except ImportError:
            from hgext_hgsubversion import util

        if self._tag_locations is None:

            tag_locations_file = os.path.join(metapath, 'tag_locations')
            self._tag_locations = util.load(tag_locations_file)

            if not self._tag_locations:
                self._tag_locations = self.meta.ui.configlist('hgsubversion',
                                                              'tagpaths',
                                                              ['tags'])
            util.dump(self._tag_locations, tag_locations_file)

            # ensure nested paths are handled properly
            self._tag_locations.sort()
            self._tag_locations.reverse()

        return self._tag_locations

    def get_path_tag(self, path, taglocations):
        for tagspath in taglocations:
            if path.startswith(tagspath + '/'):
                    tag = path[len(tagspath) + 1:]
                    if tag:
                        return tag
        return None

    def split_remote_name(self, path, known_branches):

        # this odd evolution is how we deal with people doing things like
        # creating brances (note the typo), committing to a branch under it,
        # and then moving it to branches

        # we need to find the ../foo branch names, if they exist, before
        # trying to create a normally-named branch

        components = path.split('/')
        candidate = ''
        while self.localname(candidate) not in known_branches and components:
            if not candidate:
                candidate = components.pop(0)
            else:
                candidate += '/'
                candidate += components.pop(0)
        if self.localname(candidate) in known_branches:
            return candidate, '/'.join(components)

        if path == 'trunk' or path.startswith('trunk/'):
            return self.trunk, path[len(self.trunk) + 1:]

        if path.startswith(self.meta.branchdir):
            path = path[len(self.meta.branchdir):]
            components = path.split('/', 1)
            branch_path = ''.join((self.meta.branchdir, components[0]))
            if len(components) == 1:
                local_path = ''
            else:
                local_path = components[1]

            if local_path == '':
                branch_path += self._infix
            elif local_path.startswith(self._infix[1:] + '/'):
                branch_path += self._infix
                local_path = local_path[len(self._infix):]
            return branch_path, local_path

        components = path.split('/')
        return '/'.join(components[:-1]), components[-1]
