/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include <folly/Benchmark.h>
#include <folly/init/Init.h>
#include <re2/re2.h>

#include "eden/fs/model/git/GlobMatcher.h"
#include "watchman/thirdparty/wildmatch/wildmatch.h"

using namespace facebook::eden;
using folly::StringPiece;
using std::string;

std::vector<folly::StringPiece> basenameCorpus = {
    "README",
    "README.txt",
    "test.c",
    ".test.c.swp",
    "test.h",
    "foobar.php",
    "foobar.js",
    "docs.txt",
    "BUCK",
};

std::vector<folly::StringPiece> fullnameCorpus = {
    "kernel/irq/manage.c",
    "kernel/power/console.c",
    "kernel/time/tick-internal.h",
    "include/uapi/linux/netfilter_bridge/ebt_mark_t.h",
    "README",
    "foo/README",
    "foo/test/README",
    "COPYING",
    "Documentation/DocBook/media/v4l/"
    "subdev-image-processing-scaling-multi-source.svg",
    "Documentation/DocBook/media/v4l/vidioc-g-modulator.xml",
    "Documentation/blockdev/drbd/drbd-connection-state-overview.dot",
    "Documentation/filesystems/configfs/configfs_example_explicit.c",
    "Documentation/filesystems/cifs/winucase_convert.pl",
    "net/ipv4/netfilter/nf_conntrack_l3proto_ipv4_compat.c",
    "net/netfilter/nf_conntrack_l3proto_generic.c",
};

class RE2Impl {
 public:
  RE2Impl() {}
  void init(folly::StringPiece regex) {
    re2::RE2::Options options;
    options.set_encoding(re2::RE2::Options::EncodingLatin1);
    options.set_never_nl(false);
    options.set_dot_nl(true);
    options.set_never_capture(true);
    options.set_case_sensitive(true);
    auto re2str = re2::StringPiece(regex.begin(), regex.size());
    regex_.reset(new re2::RE2(re2str, options));
  }

  bool match(folly::StringPiece input) {
    return re2::RE2::FullMatchN(
        re2::StringPiece(input.begin(), input.size()), *regex_, nullptr, 0);
  }

 private:
  std::unique_ptr<re2::RE2> regex_;
};

class GlobMatcherImpl {
 public:
  GlobMatcherImpl() {}
  void init(folly::StringPiece glob) {
    matcher_ = GlobMatcher::create(glob, GlobOptions::DEFAULT).value();
  }

  bool match(folly::StringPiece input) {
    return matcher_.match(input);
  }

 private:
  GlobMatcher matcher_;
};

class WildmatchImpl {
 public:
  WildmatchImpl() {}
  void init(folly::StringPiece glob) {
    pattern_ = glob.str();
  }

  bool match(folly::StringPiece input) {
    // wildmatch only supports null terminated strings, so we really
    // require that the StringPiece point at data that is null terminated.
    assert(input[input.size()] == '\0');
    return wildmatch(pattern_.c_str(), input.data(), WM_PATHNAME, nullptr);
  }

 private:
  std::string pattern_;
};

class FixedStringImpl {
 public:
  FixedStringImpl() {}
  void init(folly::StringPiece match) {
    pattern_ = match.str();
  }

  bool match(folly::StringPiece input) {
    return input.size() == pattern_.size() &&
        memcmp(pattern_.data(), input.data(), input.size()) == 0;
  }

 private:
  std::string pattern_;
};

class EndsWithImpl {
 public:
  EndsWithImpl() {}
  void init(folly::StringPiece match) {
    pattern_ = match.str();
  }

  bool match(folly::StringPiece input) {
    // Check that the end of the input matches the pattern
    if (input.size() > pattern_.size()) {
      return false;
    }
    if (memcmp(
            pattern_.data(), input.end() - pattern_.size(), pattern_.size()) !=
        0) {
      return false;
    }
    // To behave equivalently to the glob matching code we also have to confirm
    // that there are no slashes in the text leading up to the end.
    return memchr(input.data(), '/', input.size() - pattern_.size());
  }

 private:
  std::string pattern_;
};

template <typename Impl, typename Corpus>
void runBenchmark(size_t numIters, const char* pattern, const Corpus& corpus) {
  Impl impl;
  BENCHMARK_SUSPEND {
    impl.init(pattern);
  }

  size_t idx = 0;
  for (size_t n = 0; n < numIters; ++n) {
    auto ret = impl.match(corpus[idx]);
    folly::doNotOptimizeAway(ret);
    idx += 1;
    if (idx >= corpus.size()) {
      idx = 0;
    }
  }
}

BENCHMARK(shortFixedPath_globmatch, numIters) {
  runBenchmark<GlobMatcherImpl>(numIters, "README", basenameCorpus);
}

BENCHMARK_RELATIVE(shortFixedPath_wildmatch, numIters) {
  runBenchmark<WildmatchImpl>(numIters, "README", basenameCorpus);
}

BENCHMARK_RELATIVE(shortFixedPath_re2, numIters) {
  runBenchmark<RE2Impl>(numIters, "README", basenameCorpus);
}

BENCHMARK_RELATIVE(shortFixedPath_fixed, numIters) {
  runBenchmark<FixedStringImpl>(numIters, "README", basenameCorpus);
}

BENCHMARK(fullFixedPath_globmatch, numIters) {
  runBenchmark<GlobMatcherImpl>(numIters, "README", fullnameCorpus);
}

BENCHMARK_RELATIVE(fullFixedPath_wildmatch, numIters) {
  runBenchmark<WildmatchImpl>(numIters, "README", fullnameCorpus);
}

BENCHMARK_RELATIVE(fullFixedPath_re2, numIters) {
  runBenchmark<RE2Impl>(numIters, "README", fullnameCorpus);
}

BENCHMARK_RELATIVE(fullFixedPath_fixed, numIters) {
  runBenchmark<FixedStringImpl>(numIters, "README", fullnameCorpus);
}

BENCHMARK(endswith_globmatch, numIters) {
  runBenchmark<GlobMatcherImpl>(numIters, "*.txt", basenameCorpus);
}

BENCHMARK_RELATIVE(endswith_wildmatch, numIters) {
  runBenchmark<WildmatchImpl>(numIters, "*.txt", basenameCorpus);
}

BENCHMARK_RELATIVE(endswith_re2, numIters) {
  runBenchmark<RE2Impl>(numIters, "[^/]*\\.txt", basenameCorpus);
}

BENCHMARK_RELATIVE(endswith_fixed, numIters) {
  runBenchmark<EndsWithImpl>(numIters, ".txt", basenameCorpus);
}

BENCHMARK(basenameGlob_globmatch, numIters) {
  runBenchmark<GlobMatcherImpl>(numIters, ".*.swp", basenameCorpus);
}

BENCHMARK_RELATIVE(basenameGlob_wildmatch, numIters) {
  runBenchmark<WildmatchImpl>(numIters, ".*.swp", basenameCorpus);
}

BENCHMARK_RELATIVE(basenameGlob_re2, numIters) {
  runBenchmark<RE2Impl>(numIters, "\\.[^/]*\\.swp", basenameCorpus);
}

BENCHMARK(basenameGlob2_globmatch, numIters) {
  runBenchmark<GlobMatcherImpl>(numIters, ".*.sw?", basenameCorpus);
}

BENCHMARK_RELATIVE(basenameGlob2_wildmatch, numIters) {
  runBenchmark<WildmatchImpl>(numIters, ".*.sw?", basenameCorpus);
}

BENCHMARK_RELATIVE(basenameGlob2_re2, numIters) {
  runBenchmark<RE2Impl>(numIters, "\\.[^/]*\\.sw[^/]", basenameCorpus);
}

BENCHMARK(fullpath_globmatch, numIters) {
  runBenchmark<GlobMatcherImpl>(numIters, "**/*io*o*", fullnameCorpus);
}

BENCHMARK_RELATIVE(fullpath_wildmatch, numIters) {
  runBenchmark<WildmatchImpl>(numIters, "**/*io*o*", fullnameCorpus);
}

BENCHMARK_RELATIVE(fullpath_re2, numIters) {
  runBenchmark<RE2Impl>(numIters, ".*/[^/]io[^/]*o[^/]*", fullnameCorpus);
}

int main(int argc, char* argv[]) {
  folly::init(&argc, &argv);
  folly::runBenchmarks();
}
