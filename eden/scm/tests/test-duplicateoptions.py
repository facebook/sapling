from __future__ import absolute_import, print_function

import os

from edenscm.mercurial import commands, extensions, ui as uimod


ignore = {
    b"",
    b"factotum",
    b"fbscmquery",
    b"globalrevs",
    b"hgsql",
    b"highlight",
    b"lz4revlog",
    b"remotenames",
    b"shelve",
    b"win32text",
}

if os.name != "nt":
    ignore.add(b"win32mbcs")

disabled = [ext for ext in extensions.disabled().keys() if ext not in ignore]

hgrc = open(os.environ["HGRCPATH"], "wb")
hgrc.write(b"[extensions]\n")

for ext in disabled:
    hgrc.write(ext + b"=\n")

hgrc.close()

u = uimod.ui.load()

# Some extensions may print useful warning messages when they are loaded without
# the necessary config options. Let's capture that output since it does not
# matter for this test.
u.pushbuffer(error=True)
extensions.loadall(u)
u.popbuffer()

globalshort = set()
globallong = set()
for option in commands.globalopts:
    option[0] and globalshort.add(option[0])
    option[1] and globallong.add(option[1])

for cmd, entry in commands.table.items():
    seenshort = globalshort.copy()
    seenlong = globallong.copy()
    for option in entry[1]:
        if (option[0] and option[0] in seenshort) or (
            option[1] and option[1] in seenlong
        ):
            print("command '" + cmd + "' has duplicate option " + str(option))
        seenshort.add(option[0])
        seenlong.add(option[1])
