#!/bin/bash

hasext() {
    for modname in "$1" "hgext.$1"; do
        ${PYTHON:-python} -c "import $modname" 2> /dev/null && return 0
    done
    false
}

for extname in "$@"; do
    hasext $extname || {
        echo 'skipped: missing feature: '"$extname"
        exit 80
    }
done
