from mercurial import ui, dispatch, error

testui = ui.ui()
parsed = dispatch._parseconfig(testui, [
    'values.string=string value',
    'values.bool1=true',
    'values.bool2=false',
    'values.boolinvalid=foo',
    'values.int1=42',
    'values.int2=-42',
    'values.intinvalid=foo',
    'lists.list1=foo',
    'lists.list2=foo bar baz',
    'lists.list3=alice, bob',
    'lists.list4=foo bar baz alice, bob',
    'lists.list5=abc d"ef"g "hij def"',
    'lists.list6="hello world", "how are you?"',
    'lists.list7=Do"Not"Separate',
    'lists.list8="Do"Separate',
    'lists.list9="Do\\"NotSeparate"',
    'lists.list10=string "with extraneous" quotation mark"',
    'lists.list11=x, y',
    'lists.list12="x", "y"',
    'lists.list13=""" key = "x", "y" """',
    'lists.list14=,,,,     ',
    'lists.list15=" just with starting quotation',
    'lists.list16="longer quotation" with "no ending quotation',
    'lists.list17=this is \\" "not a quotation mark"',
    'lists.list18=\n \n\nding\ndong',
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
except error.ConfigError as inst:
    print inst
print repr(testui.configbool('values', 'bool1'))
print repr(testui.configbool('values', 'bool2'))
print repr(testui.configbool('values', 'bool2', True))
print repr(testui.configbool('values', 'unknown'))
print repr(testui.configbool('values', 'unknown', True))
print "---"
print repr(testui.configint('values', 'int1'))
print repr(testui.configint('values', 'int2'))
print "---"
print repr(testui.configlist('lists', 'list1'))
print repr(testui.configlist('lists', 'list2'))
print repr(testui.configlist('lists', 'list3'))
print repr(testui.configlist('lists', 'list4'))
print repr(testui.configlist('lists', 'list4', ['foo']))
print repr(testui.configlist('lists', 'list5'))
print repr(testui.configlist('lists', 'list6'))
print repr(testui.configlist('lists', 'list7'))
print repr(testui.configlist('lists', 'list8'))
print repr(testui.configlist('lists', 'list9'))
print repr(testui.configlist('lists', 'list10'))
print repr(testui.configlist('lists', 'list11'))
print repr(testui.configlist('lists', 'list12'))
print repr(testui.configlist('lists', 'list13'))
print repr(testui.configlist('lists', 'list14'))
print repr(testui.configlist('lists', 'list15'))
print repr(testui.configlist('lists', 'list16'))
print repr(testui.configlist('lists', 'list17'))
print repr(testui.configlist('lists', 'list18'))
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

# invalid values
try:
    testui.configbool('values', 'boolinvalid')
except error.ConfigError:
    print 'boolinvalid'
try:
    testui.configint('values', 'intinvalid')
except error.ConfigError:
    print 'intinvalid'
