import os
from mercurial import hg, ui
from mercurial.hgweb.hgwebdir_mod import hgwebdir

os.mkdir('webdir')
os.chdir('webdir')

webdir = os.path.realpath('.')

u = ui.ui()
hg.repository(u, 'a', create=1)
hg.repository(u, 'b', create=1)
os.chdir('b')
hg.repository(u, 'd', create=1)
os.chdir('..')
hg.repository(u, 'c', create=1)
os.chdir('..')

paths = {'t/a/': '%s/a' % webdir,
         'b': '%s/b' % webdir,
         'coll': '%s/*' % webdir,
         'rcoll': '%s/**' % webdir}

config = os.path.join(webdir, 'hgwebdir.conf')
configfile = open(config, 'w')
configfile.write('[paths]\n')
for k, v in paths.items():
    configfile.write('%s = %s\n' % (k, v))
configfile.close()

confwd = hgwebdir(config)
dictwd = hgwebdir(paths)

assert len(confwd.repos) == len(dictwd.repos), 'different numbers'
assert len(confwd.repos) == 9, 'expected 9 repos, found %d' % len(confwd.repos)

found = dict(confwd.repos)
for key, path in dictwd.repos:
    assert key in found, 'repository %s was not found' % key
    assert found[key] == path, 'different paths for repo %s' % key
