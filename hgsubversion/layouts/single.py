

import base

class SingleLayout(base.BaseLayout):
    """A layout with only the default branch"""

    def localname(self, path):
        return 'default'

    def remotename(self, branch):
        return ''
