from distutils.version import LooseVersion
import Cython
if LooseVersion(Cython.__version__) < LooseVersion('0.22'):
    raise RuntimeError('Cython >= 0.22 is required')

from Cython.Build import cythonize
from distutils.core import setup, Extension
from glob import glob

hgext3rd = [
    p[:-3].replace('/', '.')
    for p in glob('hgext3rd/*.py')
    if p != 'hgext3rd/__init__.py'
]

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
        'fastmanifest',
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
                      'remotefilelog/cdatapack/py-cdatapack.c',
                      'remotefilelog/cdatapack/cdatapack.c',
                  ],
                  include_dirs=[
                      'remotefilelog/cdatapack',
                      '/usr/local/include',
                      '/opt/local/include',
                  ],
                  library_dirs=[
                      '/usr/local/lib',
                      '/opt/local/lib',
                  ],
                  libraries=[
                      'crypto',
                      'lz4',
                  ],
                  extra_compile_args=[
                      "-std=c99",
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes"],
        ),
        Extension('ctreemanifest',
                  sources=[
                      'remotefilelog/ctreemanifest/py-treemanifest.cpp',
                      'remotefilelog/ctreemanifest/manifest.cpp',
                      'remotefilelog/ctreemanifest/manifest_entry.cpp',
                      'remotefilelog/ctreemanifest/manifest_fetcher.cpp',
                      'remotefilelog/ctreemanifest/pythonutil.cpp',
                      'remotefilelog/ctreemanifest/treemanifest.cpp',
                  ],
                  include_dirs=[
                      'remotefilelog/ctreemanifest',
                  ],
                  library_dirs=[
                      '/usr/local/lib',
                      '/opt/local/lib',
                  ],
                  libraries=[
                  ],
                  extra_compile_args=[
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes"],
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
                      '/usr/local/include',
                      '/opt/local/include',
                  ],
                  library_dirs=[
                      '/usr/local/lib',
                      '/opt/local/lib',
                  ],
                  libraries=['crypto',
                  ],
                  extra_compile_args=[
                      "-std=c99",
                      "-Wall",
                      "-Werror", "-Werror=strict-prototypes"],
        ),
    ] + cythonize([
        Extension('linelog',
                  sources=['linelog/pyext/linelog.pyx'],
                  extra_compile_args=[
                      '-std=c99',
                      '-Wall', '-Wextra', '-Wconversion', '-pedantic',
                  ],
        ),
    ]),
)
