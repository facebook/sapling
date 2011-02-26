# Since it's not easy to write a test that portably deals
# with files from different users/groups, we cheat a bit by
# monkey-patching some functions in the util module

import os
from mercurial import ui, util, error

hgrc = os.environ['HGRCPATH']
f = open(hgrc)
basehgrc = f.read()
f.close()

def testui(user='foo', group='bar', tusers=(), tgroups=(),
           cuser='foo', cgroup='bar', debug=False, silent=False,
           report=True):
    # user, group => owners of the file
    # tusers, tgroups => trusted users/groups
    # cuser, cgroup => user/group of the current process

    # write a global hgrc with the list of trusted users/groups and
    # some setting so that we can be sure it was read
    f = open(hgrc, 'w')
    f.write(basehgrc)
    f.write('\n[paths]\n')
    f.write('global = /some/path\n\n')

    if tusers or tgroups:
        f.write('[trusted]\n')
        if tusers:
            f.write('users = %s\n' % ', '.join(tusers))
        if tgroups:
            f.write('groups = %s\n' % ', '.join(tgroups))
    f.close()

    # override the functions that give names to uids and gids
    def username(uid=None):
        if uid is None:
            return cuser
        return user
    util.username = username

    def groupname(gid=None):
        if gid is None:
            return 'bar'
        return group
    util.groupname = groupname

    def isowner(st):
        return user == cuser
    util.isowner = isowner

    # try to read everything
    #print '# File belongs to user %s, group %s' % (user, group)
    #print '# trusted users = %s; trusted groups = %s' % (tusers, tgroups)
    kind = ('different', 'same')
    who = ('', 'user', 'group', 'user and the group')
    trusted = who[(user in tusers) + 2*(group in tgroups)]
    if trusted:
        trusted = ', but we trust the ' + trusted
    print '# %s user, %s group%s' % (kind[user == cuser], kind[group == cgroup],
                                     trusted)

    u = ui.ui()
    u.setconfig('ui', 'debug', str(bool(debug)))
    u.setconfig('ui', 'report_untrusted', str(bool(report)))
    u.readconfig('.hg/hgrc')
    if silent:
        return u
    print 'trusted'
    for name, path in u.configitems('paths'):
        print '   ', name, '=', path
    print 'untrusted'
    for name, path in u.configitems('paths', untrusted=True):
        print '.',
        u.config('paths', name) # warning with debug=True
        print '.',
        u.config('paths', name, untrusted=True) # no warnings
        print name, '=', path
    print

    return u

os.mkdir('repo')
os.chdir('repo')
os.mkdir('.hg')
f = open('.hg/hgrc', 'w')
f.write('[paths]\n')
f.write('local = /another/path\n\n')
f.close()

#print '# Everything is run by user foo, group bar\n'

# same user, same group
testui()
# same user, different group
testui(group='def')
# different user, same group
testui(user='abc')
# ... but we trust the group
testui(user='abc', tgroups=['bar'])
# different user, different group
testui(user='abc', group='def')
# ... but we trust the user
testui(user='abc', group='def', tusers=['abc'])
# ... but we trust the group
testui(user='abc', group='def', tgroups=['def'])
# ... but we trust the user and the group
testui(user='abc', group='def', tusers=['abc'], tgroups=['def'])
# ... but we trust all users
print '# we trust all users'
testui(user='abc', group='def', tusers=['*'])
# ... but we trust all groups
print '# we trust all groups'
testui(user='abc', group='def', tgroups=['*'])
# ... but we trust the whole universe
print '# we trust all users and groups'
testui(user='abc', group='def', tusers=['*'], tgroups=['*'])
# ... check that users and groups are in different namespaces
print "# we don't get confused by users and groups with the same name"
testui(user='abc', group='def', tusers=['def'], tgroups=['abc'])
# ... lists of user names work
print "# list of user names"
testui(user='abc', group='def', tusers=['foo', 'xyz', 'abc', 'bleh'],
       tgroups=['bar', 'baz', 'qux'])
# ... lists of group names work
print "# list of group names"
testui(user='abc', group='def', tusers=['foo', 'xyz', 'bleh'],
       tgroups=['bar', 'def', 'baz', 'qux'])

print "# Can't figure out the name of the user running this process"
testui(user='abc', group='def', cuser=None)

print "# prints debug warnings"
u = testui(user='abc', group='def', cuser='foo', debug=True)

print "# report_untrusted enabled without debug hides warnings"
u = testui(user='abc', group='def', cuser='foo', report=False)

print "# report_untrusted enabled with debug shows warnings"
u = testui(user='abc', group='def', cuser='foo', debug=True, report=False)

print "# ui.readconfig sections"
filename = 'foobar'
f = open(filename, 'w')
f.write('[foobar]\n')
f.write('baz = quux\n')
f.close()
u.readconfig(filename, sections = ['foobar'])
print u.config('foobar', 'baz')

print
print "# read trusted, untrusted, new ui, trusted"
u = ui.ui()
u.setconfig('ui', 'debug', 'on')
u.readconfig(filename)
u2 = u.copy()
def username(uid=None):
    return 'foo'
util.username = username
u2.readconfig('.hg/hgrc')
print 'trusted:'
print u2.config('foobar', 'baz')
print 'untrusted:'
print u2.config('foobar', 'baz', untrusted=True)

print
print "# error handling"

def assertraises(f, exc=util.Abort):
    try:
        f()
    except exc, inst:
        print 'raised', inst.__class__.__name__
    else:
        print 'no exception?!'

print "# file doesn't exist"
os.unlink('.hg/hgrc')
assert not os.path.exists('.hg/hgrc')
testui(debug=True, silent=True)
testui(user='abc', group='def', debug=True, silent=True)

print
print "# parse error"
f = open('.hg/hgrc', 'w')
f.write('foo')
f.close()

try:
    testui(user='abc', group='def', silent=True)
except error.ParseError, inst:
    print inst

try:
    testui(debug=True, silent=True)
except error.ParseError, inst:
    print inst
