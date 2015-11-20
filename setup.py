try:
    from setuptools import setup
except ImportError:
    from distutils.core import setup

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
    py_modules=[
        'automv',
        'backups',
        'chistedit',
        'commitextras',
        'dirsync',
        'fbamend',
        'fbconduit',
        'fbhistedit',
        'githelp',
        'gitlookup',
        'gitrevset',
        'inhibitwarn',
        'mergedriver',
        'morestatus',
        'perftweaks',
        'phabdiff',
        'phrevset',
        'phabstatus',
        'pushrebase',
        'pushvars',
        'rage',
        'reflog',
        'reset',
        'simplecache',
        'smartlog',
        'sparse',
        'statprof',
        'tweakdefaults',
        'upgradegeneraldelta',
        'writecg2',
    ],
    packages=[
        'copytrace',
        'crecord'
    ]
)
