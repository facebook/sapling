from distutils.core import setup, Extension

setup(
    name='fbhgext',
    version='0.1.0',
    author='Durham Goode',
    maintainer='Durham Goode',
    maintainer_email='durham@fb.com',
    url='',
    description='Facebook specific mercurial extensions',
    long_description="",
    keywords='fb hg mercurial',
    license='',
    py_modules=['fbamend', 'smartlog', 'phrevset', 'githelp', 'backups'],
    packages=['crecord']
)
