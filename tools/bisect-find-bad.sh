#!/bin/bash
. $(dirname $0)/common.sh
hg svn verify
exit $?
