#!/bin/bash

for path in "$@"; do
    [[ -e $RUNTESTDIR/../$path ]] || {
        echo 'skipped: missing core hg file: '"$path"
        exit 80
    }
done
