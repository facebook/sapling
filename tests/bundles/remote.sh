#!/bin/bash
hg init remote
cd remote

echo "0" >> afile
hg add afile
hg commit -m "0.0"
echo "1" >> afile
hg commit -m "0.1"
echo "2" >> afile
hg commit -m "0.2"
echo "3" >> afile
hg commit -m "0.3"
hg update -C 0
echo "1" >> afile
hg commit -m "1.1"
echo "2" >> afile
hg commit -m "1.2"
echo "a line" > fred
echo "3" >> afile
hg add fred
hg commit -m "1.3"
hg mv afile adifferentfile
hg commit -m "1.3m"
hg update -C 3
hg mv afile anotherfile
hg commit -m "0.3m"

hg bundle -a ../remote.hg

cd ..
rm -Rf remote
