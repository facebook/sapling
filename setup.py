#!/usr/bin/env python
#
# This is the mercurial setup script.
#
# './setup.py install', or
# './setup.py --help' for more options

import glob
from distutils.core import setup, Extension
from distutils.command.install_data import install_data

import mercurial.version

# py2exe needs to be installed to work
try:
    import py2exe

    # Due to the use of demandload py2exe is not finding the modules.
    # packagescan.getmodules creates a list of modules included in
    # the mercurial package plus depdent modules.
    import mercurial.packagescan
    from py2exe.build_exe import py2exe as build_exe

    class py2exe_for_demandload(build_exe):
        """ overwrites the py2exe command class for getting the build
        directory and for setting the 'includes' option."""
        def initialize_options(self):
            self.build_lib = None
            build_exe.initialize_options(self)
        def finalize_options(self):
            # Get the build directory, ie. where to search for modules.
            self.set_undefined_options('build',
                                       ('build_lib', 'build_lib'))
            # Sets the 'includes' option with the list of needed modules
            if not self.includes:
                self.includes = []
            self.includes += mercurial.packagescan.getmodules(self.build_lib,'mercurial')
            build_exe.finalize_options(self)
except ImportError:
    py2exe_for_demandload = None


# specify version string, otherwise 'hg identify' will be used:
version = ''

class install_package_data(install_data):
    def finalize_options(self):
        self.set_undefined_options('install',
                                   ('install_lib', 'install_dir'))
        install_data.finalize_options(self)

try:
    mercurial.version.remember_version(version)
    cmdclass = {'install_data': install_package_data}
    if py2exe_for_demandload is not None:
        cmdclass['py2exe'] = py2exe_for_demandload
    setup(name='mercurial',
          version=mercurial.version.get_version(),
          author='Matt Mackall',
          author_email='mpm@selenic.com',
          url='http://selenic.com/mercurial',
          description='scalable distributed SCM',
          license='GNU GPL',
          packages=['mercurial'],
          ext_modules=[Extension('mercurial.mpatch', ['mercurial/mpatch.c']),
                       Extension('mercurial.bdiff', ['mercurial/bdiff.c'])],
          data_files=[('mercurial/templates',
                       ['templates/map'] +
                       glob.glob('templates/map-*') +
                       glob.glob('templates/*.tmpl'))],
          cmdclass=cmdclass,
          scripts=['hg', 'hgmerge'],
          console = ['hg'])
finally:
    mercurial.version.forget_version()
