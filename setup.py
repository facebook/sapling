from distutils.ccompiler import new_compiler
from glob import glob
import os
import shutil
import tempfile

try:
    from setuptools import setup, Extensions
except ImportError:
    from distutils.core import setup, Extension

def cc_has_feature(code=None, cflags=None, ldflags=None, cc=None):
    """test a C compiler feature, return True if supported, False otherwise"""
    if code is None:
        code = 'int main() { return 0; }'
    if cflags is None:
        cflags = []
    if ldflags is None:
        ldflags = []
    if cc is None:
        cc = new_compiler()

    tmpdir = tempfile.mkdtemp(prefix='cc-feature-test')
    try:
        fname = os.path.join(tmpdir, 'a.c')
        with open(fname, 'w') as f:
            f.write(code)
        objs = cc.compile([fname], output_dir=tmpdir, extra_postargs=cflags)
        cc.link_executable(objs, os.path.join(tmpdir, 'a'),
                           extra_postargs=ldflags)
        return True
    except Exception:
        return False
    finally:
        shutil.rmtree(tmpdir)

hgext3rd = [
    p[:-3].replace('/', '.')
    for p in glob('hgext3rd/*.py')
    if p != 'hgext3rd/__init__.py'
]

setup(
    name='fbhgext',
    version='0.1.2',
    author='Durham Goode',
    maintainer='Durham Goode',
    maintainer_email='durham@fb.com',
    url='',
    description='Facebook specific mercurial extensions',
    long_description="",
    keywords='fb hg mercurial',
    license='',
    packages=[
        'fastmanifest',
        'phabricator',
        'sqldirstate',
    ],
    py_modules=[
        'statprof'
    ] + hgext3rd,
    ext_modules = [
        Extension('cfastmanifest',
                  sources=['cfastmanifest.c',
                           'cfastmanifest/bsearch.c',
                           'cfastmanifest/buffer.c',
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
                  include_dirs=['cfastmanifest',
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
        )
    ],
)
