#!/bin/bash
hg init rebase
cd rebase

echo A > A
hg ci -Am A
echo B > B
hg ci -Am B
hg up -q -C 0
echo C > C
hg ci -Am C
hg up -q -C 0
echo D > D
hg ci -Am D
hg merge -r 2
hg ci -m E
hg up -q -C 3
echo F > F
hg ci -Am F

hg bundle -a ../rebase.hg

cd ..
rm -Rf rebase
