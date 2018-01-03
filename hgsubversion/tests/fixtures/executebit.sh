#!/bin/sh
#
# Generate executebit.svndump
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
echo text > text1
echo text > text2
touch empty1
touch empty2
python -c "file('binary1', 'wb').write('a\x00b')"
python -c "file('binary2', 'wb').write('a\x00b')"
svn add text1 text2 binary1 binary2 empty1 empty2
svn propset svn:mime-type application/octet-stream binary1 binary2
svn propset svn:executable yes binary1 text1 empty1
svn ci -m init
# switch exec properties
svn propdel svn:executable binary1 text1 empty1
svn propset svn:executable yes binary2 text2 empty2
svn ci -m changeexec
cd ../..

svnadmin dump testrepo > ../executebit.svndump
