#!/usr/bin/env python

from mercurial import ui, util, dispatch, error

testui = ui.ui()
parsed = dispatch._parseconfig(testui, [
    'values.string=string value',
    'values.bool1=true',
    'values.bool2=false',
    'lists.list1=foo',
    'lists.list2=foo bar baz',
    'lists.list3=alice, bob',
    'lists.list4=foo bar baz alice, bob',
])

print repr(testui.configitems('values'))
print repr(testui.configitems('lists'))
print "---"
print repr(testui.config('values', 'string'))
print repr(testui.config('values', 'bool1'))
print repr(testui.config('values', 'bool2'))
print repr(testui.config('values', 'unknown'))
print "---"
try:
    print repr(testui.configbool('values', 'string'))
except error.ConfigError, inst:
    print inst
print repr(testui.configbool('values', 'bool1'))
print repr(testui.configbool('values', 'bool2'))
print repr(testui.configbool('values', 'bool2', True))
print repr(testui.configbool('values', 'unknown'))
print repr(testui.configbool('values', 'unknown', True))
print "---"
print repr(testui.configlist('lists', 'list1'))
print repr(testui.configlist('lists', 'list2'))
print repr(testui.configlist('lists', 'list3'))
print repr(testui.configlist('lists', 'list4'))
print repr(testui.configlist('lists', 'list4', ['foo']))
print repr(testui.configlist('lists', 'unknown'))
print repr(testui.configlist('lists', 'unknown', ''))
print repr(testui.configlist('lists', 'unknown', 'foo'))
print repr(testui.configlist('lists', 'unknown', ['foo']))
print repr(testui.configlist('lists', 'unknown', 'foo bar'))
print repr(testui.configlist('lists', 'unknown', 'foo, bar'))
print repr(testui.configlist('lists', 'unknown', ['foo bar']))
print repr(testui.configlist('lists', 'unknown', ['foo', 'bar']))

print repr(testui.config('values', 'String'))

def function():
    pass

# values that aren't strings should work
testui.setconfig('hook', 'commit', function)
print function == testui.config('hook', 'commit')
