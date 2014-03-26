#!/usr/bin/env bash

# Measure the performance of a list of revsets against multiple revisions
# defined by parameter. Checkout one by one and run perfrevset with every
# revset in the list to benchmark its performance.
#
# - First argument is a revset of mercurial own repo to runs against.
# - Second argument is the file from which the revset array will be taken
#   If second argument is omitted read it from standard input
#
# You should run this from the root of your mercurial repository.
#
# This script also does one run of the current version of mercurial installed
# to compare performance.

HG="hg update"
PERF="./hg --config extensions.perf=contrib/perf.py perfrevset"
BASE_PERF="hg --config extensions.perf=contrib/perf.py perfrevset"

TARGETS=$1
shift
# read from a file or from standard output
if [ $# -ne 0 ]; then
    readarray REVSETS < $1
else
    readarray REVSETS
fi

hg update --quiet

echo "Starting time benchmarking"
echo

echo "Revsets to benchmark"
echo "----------------------------"

for (( j = 0; j < ${#REVSETS[@]}; j++ ));
do
  echo "${j}) ${REVSETS[$j]}"
done

echo "----------------------------"
echo

# Benchmark baseline
echo "Benchmarking baseline"

for (( j = 0; j < ${#REVSETS[@]}; j++ ));
  do
    echo -n "${j}) "
    $BASE_PERF "${REVSETS[$j]}"
done

echo
echo

# Benchmark revisions
for i in $(hg log --template='{rev}\n' --rev $TARGETS);
do
  echo "----------------------------"
  echo -n "Revision: "
  hg log -r $i --template "{desc|firstline}\n"

  echo "----------------------------"
  $HG $i
  for (( j = 0; j < ${#REVSETS[@]}; j++ ));
  do
    echo -n "${j}) "
    $PERF "${REVSETS[$j]}"
  done
  echo "----------------------------"
done

$HG

# Benchmark current code
echo "Benchmarking current code"

for (( j = 0; j < ${#REVSETS[@]}; j++ ));
  do
    echo -n "${j}) "
    $PERF "${REVSETS[$j]}"
done


echo
echo "Time benchmarking finished"


