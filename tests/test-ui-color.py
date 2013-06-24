import os
from hgext import color
from mercurial import dispatch, ui

# ensure errors aren't buffered
testui = color.colorui()
testui.pushbuffer()
testui.write(('buffered\n'))
testui.warn(('warning\n'))
testui.write_err('error\n')
print repr(testui.popbuffer())

# test dispatch.dispatch with the same ui object
hgrc = open(os.environ["HGRCPATH"], 'w')
hgrc.write('[extensions]\n')
hgrc.write('color=\n')
hgrc.close()

ui_ = ui.ui()
ui_.setconfig('ui', 'formatted', 'True')

# we're not interested in the output, so write that to devnull
ui_.fout = open(os.devnull, 'w')

# call some arbitrary command just so we go through
# color's wrapped _runcommand twice.
def runcmd():
    dispatch.dispatch(dispatch.request(['version', '-q'], ui_))

runcmd()
print "colored? " + str(issubclass(ui_.__class__, color.colorui))
runcmd()
print "colored? " + str(issubclass(ui_.__class__, color.colorui))
