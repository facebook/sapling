import os.path
import pickle

import base
import hgsubversion.util as util

class StandardLayout(base.BaseLayout):
    """The standard trunk, branches, tags layout"""

    def __init__(self, ui):
        base.BaseLayout.__init__(self, ui)

        self._tag_locations = None

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
        if subdir == '/':
            subdir = ''
        branchpath = 'trunk'
        if branch and branch != 'default':
            if branch.startswith('../'):
                branchpath = branch[3:]
            else:
                branchpath = 'branches/%s' % branch

        return '%s/%s' % (subdir or '', branchpath)

    def taglocations(self, meta_data_dir):
        if self._tag_locations is None:

            tag_locations_file = os.path.join(meta_data_dir, 'tag_locations')

            if os.path.exists(tag_locations_file):
                f = open(tag_locations_file)
                self._tag_locations = pickle.load(f)
                f.close()
            else:
                self._tag_locations = self.ui.configlist('hgsubversion',
                                                        'tagpaths',
                                                        ['tags'])
            util.pickle_atomic(self._tag_locations, tag_locations_file)

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
            branch_path = 'trunk'
            local_path = '/'.join(path.split('/')[1:])
        elif path.startswith('branches/'):
            components = path.split('/')
            branch_path = '/'.join(components[:2])
            local_path = '/'.join(components[2:])
        else:
            components = path.split('/')
            branch_path = '/'.join(components[:-1])
            local_path = components[-1]
        return branch_path, local_path
