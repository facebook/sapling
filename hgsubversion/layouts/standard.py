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

        # infix is expected to be stripped of trailing slashes but retain
        # its first slash
        def _infix_transform(x):
            x = '/'.join(p for p in x.split('/') if p)
            if x:
                x = '/' + x
            return x
        meta._gen_cachedconfig('infix', '', pre=_infix_transform)

        # the lambda is to ensure nested paths are handled properly
        meta._gen_cachedconfig('taglocations', ['tags'], 'tag_locations',
                               'tagpaths', lambda x: list(reversed(sorted(x))))
        meta._gen_cachedconfig('trunkdir', 'trunk', 'trunk_dir')

    @property
    def name(self):
        return 'standard'

    @property
    def trunk(self):
        return self.meta.trunkdir + self.meta.infix

    def localname(self, path):
        if path == self.trunk:
            return None
        elif path.startswith(self.meta.branchdir) and path.endswith(self.meta.infix):
            path = path[len(self.meta.branchdir):]
            if self.meta.infix:
                path = path[:-len(self.meta.infix)]
            return path
        return  '../%s' % path

    def remotename(self, branch):
        if branch == 'default' or branch is None:
            path = self.trunk
        elif branch.startswith('../'):
            path =  branch[3:]
        else:
            path = ''.join((self.meta.branchdir, branch, self.meta.infix))

        return path

    def remotepath(self, branch, subdir='/'):
        if subdir == '/':
            subdir = ''
        branchpath = self.trunk
        if branch and branch != 'default':
            if branch.startswith('../'):
                branchpath = branch[3:]
            else:
                branchpath = ''.join((self.meta.branchdir, branch,
                                      self.meta.infix))

        return '%s/%s' % (subdir or '', branchpath)

    @property
    def taglocations(self):
        return self.meta.taglocations

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

        if path == self.meta.trunkdir or path.startswith(self.meta.trunkdir + '/'):
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
                branch_path += self.meta.infix
            elif local_path.startswith(self.meta.infix[1:] + '/'):
                branch_path += self.meta.infix
                local_path = local_path[len(self.meta.infix):]
            return branch_path, local_path

        components = path.split('/')
        return '/'.join(components[:-1]), components[-1]
