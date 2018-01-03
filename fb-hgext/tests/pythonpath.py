from __future__ import absolute_import

import glob
import os
import sys

reporoot = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def globrelative(path):
    return glob.glob(os.path.join(reporoot, path))

def setcstorepath():
    sys.path[0:0] = (
        # make local
        [reporoot] +
        # python2 setup.py build_ext
        globrelative('build/lib*') +
        # rpmbuild
        globrelative('../rpmbuild/BUILD/fb-mercurial-ext-*/build/lib.*')
    )

