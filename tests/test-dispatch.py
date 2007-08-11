import os
from mercurial import commands

def dispatch(cmd):
    """Simple wrapper around commands.dispatch()

    Prints command and result value, but does not handle quoting.
    """
    print "running: %s" % (cmd,)
    result = commands.dispatch(cmd.split())
    print "result: %r" % (result,)


dispatch("init test1")
os.chdir('test1')

# create file 'foo', add and commit
f = file('foo', 'wb')
f.write('foo\n')
f.close()
dispatch("add foo")
dispatch("commit -m commit1 -d 2000-01-01 foo")

# append to file 'foo' and commit
f = file('foo', 'ab')
f.write('bar\n')
f.close()
dispatch("commit -m commit2 -d 2000-01-02 foo")

# check 88803a69b24 (fancyopts modified command table)
dispatch("log -r 0")
dispatch("log -r tip")
