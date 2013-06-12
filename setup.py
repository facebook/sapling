from distutils.core import setup, Extension

setup(
    name='hgshallowrepo',
    version='0.1',
    author='Durham Goode',
    maintainer='Durham Goode',
    maintainer_email='durham@fb.com',
    url='https://bitbucket.org/facebook/hgshallowrepo',
    description='Shallow repo extension for Mercurial',
    long_description="""
This extension adds support for shallow repositories in Mercurial where all the file history is stored remotely.
    """.strip(),
    keywords='hg shallow mercurial',
    license='Not determined yet',
    packages=['hgshallowrepo'],
    ext_modules = []
)
