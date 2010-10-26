import os
from mercurial import ui

hgrc = os.environ['HGRCPATH']
f = open(hgrc)
basehgrc = f.read()
f.close()

print '      hgrc settings    command line options      final result   '
print '    quiet verbo debug   quiet verbo debug      quiet verbo debug'

for i in xrange(64):
    hgrc_quiet   = bool(i & 1<<0)
    hgrc_verbose = bool(i & 1<<1)
    hgrc_debug   = bool(i & 1<<2)
    cmd_quiet    = bool(i & 1<<3)
    cmd_verbose  = bool(i & 1<<4)
    cmd_debug    = bool(i & 1<<5)

    f = open(hgrc, 'w')
    f.write(basehgrc)
    f.write('\n[ui]\n')
    if hgrc_quiet:
        f.write('quiet = True\n')
    if hgrc_verbose:
        f.write('verbose = True\n')
    if hgrc_debug:
        f.write('debug = True\n')
    f.close()

    u = ui.ui()
    if cmd_quiet or cmd_debug or cmd_verbose:
        u.setconfig('ui', 'quiet', str(bool(cmd_quiet)))
        u.setconfig('ui', 'verbose', str(bool(cmd_verbose)))
        u.setconfig('ui', 'debug', str(bool(cmd_debug)))

    check = ''
    if u.debugflag:
        if not u.verbose or u.quiet:
            check = ' *'
    elif u.verbose and u.quiet:
        check = ' +'

    print ('%2d  %5s %5s %5s   %5s %5s %5s  ->  %5s %5s %5s%s'
           % (i, hgrc_quiet, hgrc_verbose, hgrc_debug,
              cmd_quiet, cmd_verbose, cmd_debug,
              u.quiet, u.verbose, u.debugflag, check))
