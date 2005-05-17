#!/usr/bin/env python

# This is the mercurial setup script. 
#
# './setup.py install', or
# './setup.py --help' for more options

from distutils.core import setup, Extension

setup(name='mercurial',
      version='0.4f',
      author='Matt Mackall',
      author_email='mpm@selenic.com',
      url='http://selenic.com/mercurial',
      description='scalable distributed SCM',
      license='GNU GPL',
      packages=['mercurial'],
      ext_modules=[Extension('mercurial.mpatch', ['mercurial/mpatch.c'])],
      scripts=['hg', 'hgweb.py'])
