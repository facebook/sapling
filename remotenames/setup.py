try:
    from setuptools import setup
except ImportError:
    from distutils.core import setup

setup(
    name='remotenames',
    version='0.2',
    author='Sean Farley',
    maintainer='Sean Farley',
    maintainer_email='sean@farley.io',
    url='https://bitbucket.org/seanfarley/hgremotenames',
    description='Mark remote branch and bookmark heads in Mercurial',
    long_description="""
This extension automatically creates a local tag-like marker
during a pull from a remote server that has its path specifed
in .hg/hgrc.
    """.strip(),
    keywords='hg mercurial',
    license='GPLv2',
    py_modules=['remotenames'],
)
