

import base

class SingleLayout(base.BaseLayout):
    """A layout with only the default branch"""

    def localname(self, path):
        return None

    def remotename(self, branch):
        return ''

    def remotepath(self, branch, subdir='/'):
        return subdir or '/'

    def taglocations(self, metapath):
        return []

    def get_path_tag(self, path, taglocations):
        return None

    def split_remote_name(self, path, known_branches):
        return '', path
