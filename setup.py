try:
    from setuptools import setup
except:
    from distutils.core import setup

setup(
    name='hg-remotebranches',
    version='1.0.0',
    author='Augie Fackler',
    maintainer='Augie Fackler',
    maintainer_email='durin42@gmail.com',
#    url='',
    description='Mark remote branch heads in Mercurial',
    long_description="""
This extension automatically creates a local tag-like marker
during a pull from a remote server that has its path specifed
in .hg/hgrc.
    """.strip(),
    keywords='hg mercurial',
    license='GPLv2',
    py_modules=['hg_remotebranches'],
)
