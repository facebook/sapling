#!/usr/bin/env python

# This is the mercurial setup script. 
#
# './setup.py install', or
# './setup.py --help' for more options

from distutils.core import setup

setup(name='mercurial',
            version='0.4d',
            author='Matt Mackall',
            author_email='mpm@selenic.com',
            url='http://selenic.com/mercurial',
            description='scalable distributed SCM',
            license='GNU GPL',
            packages=['mercurial'],
            scripts=['hg'])
