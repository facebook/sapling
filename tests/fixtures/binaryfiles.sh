#!/bin/sh
#
# Generate binaryfiles.svndump
#

mkdir temp
cd temp

mkdir project-orig
cd project-orig
mkdir trunk
cd ..

svnadmin create testrepo
svnurl=file://`pwd`/testrepo
svn import project-orig $svnurl -m "init project"

svn co $svnurl project
cd project/trunk
# Add a regular binary file, and an unflagged one
python -c "file('binary1', 'wb').write('a\0\0\nb\0b')"
python -c "file('binary2', 'wb').write('b\0\0\nc\0d')"
svn add binary1 binary2
svn propset svn:mime-type application/octet-stream binary1
svn propdel svn:mime-type binary2
svn ci -m 'add binaries'
# Update them
python -c "file('binary1', 'wb').write('a\0\0\nc\0d')"
python -c "file('binary2', 'wb').write('b\0\0\0\nd\0e')"
svn ci -m 'change binaries'
# Remove them
svn rm binary1 binary2
svn ci -m 'remove binaries'
cd ../..

svnadmin dump testrepo > ../binaryfiles.svndump
