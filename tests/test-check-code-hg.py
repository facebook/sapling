# Pass all working directory files through check-code.py

import sys, os, imp
rootdir = os.path.abspath(os.path.join(os.path.dirname(sys.argv[0]), '..'))
if not os.path.isdir(os.path.join(rootdir, '.hg')):
    sys.stderr.write('skipped: cannot check code on non-repository sources\n')
    sys.exit(80)

checkpath = os.path.join(rootdir, 'contrib/check-code.py')
checkcode = imp.load_source('checkcode', checkpath)

from mercurial import hg, ui
u = ui.ui()
repo = hg.repository(u, rootdir)
checked = 0
wctx = repo[None]
for f in wctx:
    # ignore removed and unknown files
    if f not in wctx:
        continue
    checked += 1
    checkcode.checkfile(os.path.join(rootdir, f))
if not checked:
    sys.stderr.write('no file checked!\n')
