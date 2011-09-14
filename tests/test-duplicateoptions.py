import os
from mercurial import ui, commands, extensions

ignore = set(['highlight', 'inotify', 'win32text'])

if os.name != 'nt':
    ignore.add('win32mbcs')

disabled = [ext for ext in extensions.disabled().keys() if ext not in ignore]

hgrc = open(os.environ["HGRCPATH"], 'w')
hgrc.write('[extensions]\n')

for ext in disabled:
    hgrc.write(ext + '=\n')

hgrc.close()

u = ui.ui()
extensions.loadall(u)

globalshort = set()
globallong = set()
for option in commands.globalopts:
    option[0] and globalshort.add(option[0])
    option[1] and globallong.add(option[1])

for cmd, entry in commands.table.iteritems():
    seenshort = globalshort.copy()
    seenlong = globallong.copy()
    for option in entry[1]:
        if (option[0] and option[0] in seenshort) or \
           (option[1] and option[1] in seenlong):
            print "command '" + cmd + "' has duplicate option " + str(option)
        seenshort.add(option[0])
        seenlong.add(option[1])
