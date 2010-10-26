from hgext import color

# ensure errors aren't buffered
testui = color.colorui()
testui.pushbuffer()
testui.write('buffered\n')
testui.warn('warning\n')
testui.write_err('error\n')
print repr(testui.popbuffer())
