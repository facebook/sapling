from distutils.version import LooseVersion
import Cython
if LooseVersion(Cython.__version__) < LooseVersion('0.22'):
    raise RuntimeError('Cython >= 0.22 is required')

from Cython.Build import cythonize
from distutils.core import setup, Extension
from glob import glob

import os

hgext3rd = [
    p[:-3].replace('/', '.')
    for p in glob('hgext3rd/*.py')
    if p != 'hgext3rd/__init__.py'
]

# if this is set, compile all C extensions with -O0 -g for easy debugging.  note
# that this is not manifested in any way in the Makefile dependencies.
# therefore, if you already have build products, they won't be rebuilt!
if os.getenv('FB_HGEXT_CDEBUG') is not None:
    cdebugflags = ["-O0", "-g"]
else:
    cdebugflags = []

def get_env_path_list(var_name, default=None):
    '''Get a path list from an environment variable.  The variable is parsed as
    a colon-separated list.'''
    value = os.environ.get(var_name)
    if not value:
        return default
    return value.split(os.path.pathsep)

include_dirs = get_env_path_list('INCLUDE_DIRS')
library_dirs = get_env_path_list('LIBRARY_DIRS')

# Historical default values.
# We should perhaps clean these up in the future after verifying that it
# doesn't break the build on any platforms.
#
# The /usr/local/* directories shouldn't actually be needed--the compiler
# should already use these directories when appropriate (e.g., if we are
# using the standard system compiler that has them in its default paths).
#
# The /opt/local paths may be necessary on Darwin builds.
if include_dirs is None:
    include_dirs = ['/usr/local/include', '/opt/local/include']
if library_dirs is None:
    library_dirs = ['/usr/local/lib', '/opt/local/lib']

setup(
    name='fbhgext',
    version='1.0',
    author='Facebook Source Control Team',
    maintainer='Facebook Source Control Team',
    maintainer_email='sourcecontrol-dev@fb.com',
    url='https://bitbucket.org/facebook/hg-experimental/',
    description='Facebook mercurial extensions',
    long_description="",
    keywords='facebook fb hg mercurial shallow remote filelog',
    license='GPLv2+',
    packages=[
        'fastannotate',
        'fastmanifest',
        'infinitepush',
        'phabricator',
        'sqldirstate',
        'remotefilelog',
    ],
    install_requires=['lz4'],
    py_modules=[
        'statprof'
    ] + hgext3rd,
    ext_modules = [
        Extension('cdatapack',
                  sources=[
                      'cdatapack/py-cdatapack.c',
                      'cdatapack/cdatapack.c',
                  ],
                  include_dirs=[
                      'clib',
                      'cdatapack',
                  ] + include_dirs,
                  library_dirs=library_dirs,
                  libraries=[
                      'crypto',
                      'lz4',
                  ],
                  extra_compile_args=[
                      "-std=c99",
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes",
                  ] + cdebugflags,
        ),
        Extension('ctreemanifest',
                  sources=[
                      'ctreemanifest/py-treemanifest.cpp',
                      'ctreemanifest/manifest.cpp',
                      'ctreemanifest/manifest_entry.cpp',
                      'ctreemanifest/manifest_fetcher.cpp',
                      'ctreemanifest/pythonutil.cpp',
                      'ctreemanifest/treemanifest.cpp',
                  ],
                  include_dirs=[
                      'ctreemanifest',
                  ] + include_dirs,
                  library_dirs=library_dirs,
                  libraries=[
                      'crypto',
                  ],
                  extra_compile_args=[
                      "-std=c++0x",
                      "-Wall",
                      "-Werror",
                  ] + cdebugflags,
        ),
        Extension('cfastmanifest',
                  sources=['cfastmanifest.c',
                           'cfastmanifest/bsearch.c',
                           'clib/buffer.c',
                           'cfastmanifest/checksum.c',
                           'cfastmanifest/node.c',
                           'cfastmanifest/tree.c',
                           'cfastmanifest/tree_arena.c',
                           'cfastmanifest/tree_convert.c',
                           'cfastmanifest/tree_copy.c',
                           'cfastmanifest/tree_diff.c',
                           'cfastmanifest/tree_disk.c',
                           'cfastmanifest/tree_iterator.c',
                           'cfastmanifest/tree_path.c',
                  ],
                  include_dirs=[
                      'cfastmanifest',
                      'clib',
                  ] + include_dirs,
                  library_dirs=library_dirs,
                  libraries=['crypto',
                  ],
                  extra_compile_args=[
                      "-std=c99",
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes",
                  ] + cdebugflags,
        ),
    ] + cythonize([
        Extension('linelog',
                  sources=['linelog/pyext/linelog.pyx'],
                  extra_compile_args=[
                      '-std=c99',
                      '-Wall', '-Wextra', '-Wconversion', '-pedantic',
                  ] + cdebugflags,
        ),
    ]),
)
