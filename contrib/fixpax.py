# fixpax - fix ownership in bdist_mpkg output
#
# Copyright 2015 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# MIT license (http://opensource.org/licenses/MIT)

"""Set file ownership to 0 in an Archive.pax.gz.
Suitable for fixing files bdist_mpkg output:
*.mpkg/Contents/Packages/*.pkg/Contents/Archive.pax.gz
"""

import sys, os, gzip

def fixpax(iname, oname):
    i = gzip.GzipFile(iname)
    o = gzip.GzipFile(oname, "w")

    while True:
        magic = i.read(6)
        dev = i.read(6)
        ino = i.read(6)
        mode = i.read(6)
        i.read(6) # uid
        i.read(6) # gid
        nlink = i.read(6)
        rdev = i.read(6)
        mtime = i.read(11)
        namesize = i.read(6)
        filesize = i.read(11)
        name = i.read(int(namesize, 8))
        data = i.read(int(filesize, 8))

        o.write(magic)
        o.write(dev)
        o.write(ino)
        o.write(mode)
        o.write("000000")
        o.write("000000")
        o.write(nlink)
        o.write(rdev)
        o.write(mtime)
        o.write(namesize)
        o.write(filesize)
        o.write(name)
        o.write(data)

        if name.startswith("TRAILER!!!"):
            o.write(i.read())
            break

    o.close()
    i.close()

if __name__ == '__main__':
    for iname in sys.argv[1:]:
        print 'fixing file ownership in %s' % iname
        oname = sys.argv[1] + '.tmp'
        fixpax(iname, oname)
        os.rename(oname, iname)
