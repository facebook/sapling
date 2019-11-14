#!/bin/sh

RSVN="`pwd`/rsvn.py"
export PATH=/bin:/usr/bin
mkdir temp
cd temp

svnadmin create repo
svn co file://`pwd`/repo wc

cd wc
mkdir trunk branches
cd trunk
echo a > a
mkdir d
echo b > d/b
ln -s d dlink
ln -s d dlink2
ln -s d dlink3
mkdir d2
echo a > d2/a
cd ..
svn add *
svn ci -m 'initial'
# Clobber symlink with file with similar content
cd trunk
ls -Alh
readlink dlink3 > dlink3tmp
rm dlink3
mv dlink3tmp dlink3
svn propdel svn:special dlink3
svn ci -m 'clobber symlink'
cd ..
svn up

# Clobber files and symlink with directories
cd ..
cat > clobber.rsvn <<EOF
rdelete trunk/a
rdelete trunk/dlink
rcopy trunk/d trunk/a
rcopy trunk/d trunk/dlink
EOF

python $RSVN --message=clobber1 --username=evil `pwd`/repo < clobber.rsvn

# Clobber non-symlink with symlink with same content (kudos openwrt)
cat > clobber.rsvn <<EOF
rdelete trunk/dlink3
rcopy trunk/dlink2 trunk/dlink3
EOF

python $RSVN --message=clobber2 --username=evil `pwd`/repo < clobber.rsvn

# Create d2 in branch so d2 has 'a' is in branch/d2 and trunk/d2,
# 'b' is in trunk/d2 and 'c' is in branch/d2
cd wc/trunk
echo b > d2/b
svn add d2/b
svn ci -m adddb
cd ..
svn up
svn cp trunk branches/branch
cd branches/branch
svn rm d2/b
echo c > d2/c
svn add d2/c
cd ../..
svn ci -m branch
svn up
cd ..

cat > clobber.rsvn <<EOF
rdelete trunk/d2
rcopy branches/branch/d2 trunk/d2
EOF
python $RSVN --message=clobberdir --username=evil `pwd`/repo < clobber.rsvn

svn log -v file://`pwd`/repo

svnadmin dump repo > ../replace.svndump
