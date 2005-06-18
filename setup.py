#!/usr/bin/env python

# This is the mercurial setup script. 
#
# './setup.py install', or
# './setup.py --help' for more options

import glob
from distutils.core import setup, Extension
from distutils.command.install_data import install_data

import mercurial.version

class install_package_data(install_data):
    def finalize_options(self):
        self.set_undefined_options('install',
                                   ('install_lib', 'install_dir'))
        install_data.finalize_options(self)

try:
    mercurial.version.remember_version()
    setup(name='mercurial',
        version=mercurial.version.get_version(),
        author='Matt Mackall',
        author_email='mpm@selenic.com',
        url='http://selenic.com/mercurial',
        description='scalable distributed SCM',
        license='GNU GPL',
        packages=['mercurial'],
        ext_modules=[Extension('mercurial.mpatch', ['mercurial/mpatch.c'])],
        data_files=[('mercurial/templates',
                    ['templates/map'] +
                    glob.glob('templates/map-*') +
                    glob.glob('templates/*.tmpl'))], 
        cmdclass = { 'install_data' : install_package_data },
        scripts=['hg', 'hgmerge'])
finally:
    mercurial.version.forget_version()
