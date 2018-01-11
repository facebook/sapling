# make "hg version" print the omnibus package version

from mercurial import (
    commands,
    extensions,
)

# will be replaced by setup.py
release = '@RELEASE@'

def moreversion(orig, ui, **opts):
    # insert release string at the second line
    ui.pushbuffer()
    orig(ui, **opts)
    lines = ui.popbuffer().splitlines()
    lines[1:1] = ['Facebook Mercurial release: %s\n' % release]
    ui.write(lines[0] + '\n')
    ui.status('\n'.join(lines[1:]) + '\n')

def uisetup(ui):
    extensions.wrapcommand(commands.table, 'version', moreversion)
    extensions.wrapfunction(commands, 'version_', moreversion)
