

import base


class StandardLayout(base.BaseLayout):
    """The standard trunk, branches, tags layout"""

    def localname(self, path):
        if path == 'trunk':
            return None
        elif path.startswith('branches/'):
            return path[len('branches/'):]
        return  '../%s' % path
