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
        branchpath = 'trunk'
        if branch:
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
