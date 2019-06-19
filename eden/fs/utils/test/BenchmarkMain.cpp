/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/Benchmark.h>
#include <folly/init/Init.h>

int main(int argc, char** argv) {
  folly::init(&argc, &argv);
  folly::runBenchmarks();
}
