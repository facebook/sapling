/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <benchmark/benchmark.h>
#include <re2/re2.h>
#include <string.h>

#include "eden/fs/model/git/GlobMatcher.h"
#include "watchman/thirdparty/wildmatch/wildmatch.h"

using namespace facebook::eden;

std::vector<std::string> basenameCorpus = {
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

std::vector<std::string> fullnameCorpus = {
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
  void init(std::string_view regex, CaseSensitivity caseSensitive) {
    re2::RE2::Options options;
    options.set_encoding(re2::RE2::Options::EncodingLatin1);
    options.set_never_nl(false);
    options.set_dot_nl(true);
    options.set_never_capture(true);
    options.set_case_sensitive(caseSensitive == CaseSensitivity::Sensitive);
    auto re2str = re2::StringPiece(regex.data(), regex.size());
    regex_.reset(new re2::RE2(re2str, options));
  }

  bool match(std::string_view input) {
    return re2::RE2::FullMatchN(
        re2::StringPiece(input.data(), input.size()), *regex_, nullptr, 0);
  }

 private:
  std::unique_ptr<re2::RE2> regex_;
};

class GlobMatcherImpl {
 public:
  GlobMatcherImpl() {}
  void init(std::string_view glob, CaseSensitivity caseSensitive) {
    matcher_ = GlobMatcher::create(
                   glob,
                   caseSensitive == CaseSensitivity::Insensitive
                       ? GlobOptions::CASE_INSENSITIVE
                       : GlobOptions::DEFAULT)
                   .value();
  }

  bool match(const std::string& input) {
    return matcher_.match(input);
  }

 private:
  GlobMatcher matcher_;
};

class WildmatchImpl {
 public:
  WildmatchImpl() {}
  void init(std::string_view glob, CaseSensitivity caseSensitive) {
    pattern_ = glob;
    flags_ = WM_PATHNAME |
        ((caseSensitive == CaseSensitivity::Insensitive) ? WM_CASEFOLD : 0);
  }

  bool match(const std::string& input) {
    return wildmatch(pattern_.c_str(), input.c_str(), flags_, nullptr);
  }

 private:
  std::string pattern_;
  int flags_;
};

class FixedStringImpl {
 public:
  FixedStringImpl() {}
  void init(std::string_view match, CaseSensitivity caseSensitive) {
    pattern_ = match;
    assert(caseSensitive == CaseSensitivity::Sensitive);
    (void)caseSensitive;
  }

  bool match(const std::string& input) {
    return input.size() == pattern_.size() &&
        memcmp(pattern_.data(), input.data(), input.size()) == 0;
  }

 private:
  std::string pattern_;
};

class EndsWithImpl {
 public:
  EndsWithImpl() {}
  void init(std::string_view match, CaseSensitivity caseSensitive) {
    pattern_ = match;
    assert(caseSensitive == CaseSensitivity::Sensitive);
    (void)caseSensitive;
  }

  bool match(std::string_view input) {
    // Check that the end of the input matches the pattern
    if (input.size() > pattern_.size()) {
      return false;
    }
    if (memcmp(
            pattern_.data(),
            input.data() + input.size() - pattern_.size(),
            pattern_.size()) != 0) {
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
void runBenchmark(
    benchmark::State& state,
    const char* pattern,
    const Corpus& corpus,
    CaseSensitivity caseSensitive = CaseSensitivity::Sensitive) {
  Impl impl;
  impl.init(pattern, caseSensitive);

  size_t idx = 0;
  for (auto _ : state) {
    auto ret = impl.match(corpus[idx]);
    benchmark::DoNotOptimize(ret);
    idx += 1;
    if (idx >= corpus.size()) {
      idx = 0;
    }
  }
}

#define GBENCHMARK(name)                 \
  static void name(::benchmark::State&); \
  BENCHMARK(name);                       \
  void name

GBENCHMARK(shortFixedPath_globmatch)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(state, "README", basenameCorpus);
}

GBENCHMARK(shortFixedPath_globmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(
      state, "README", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(shortFixedPath_wildmatch)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(state, "README", basenameCorpus);
}

GBENCHMARK(shortFixedPath_wildmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(
      state, "README", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(shortFixedPath_re2)(benchmark::State& state) {
  runBenchmark<RE2Impl>(state, "README", basenameCorpus);
}

GBENCHMARK(shortFixedPath_re2_case_insensitive)(benchmark::State& state) {
  runBenchmark<RE2Impl>(
      state, "README", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(shortFixedPath_fixed)(benchmark::State& state) {
  runBenchmark<FixedStringImpl>(state, "README", basenameCorpus);
}

GBENCHMARK(fullFixedPath_globmatch)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(state, "README", fullnameCorpus);
}

GBENCHMARK(fullFixedPath_globmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(
      state, "README", fullnameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(fullFixedPath_wildmatch)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(state, "README", fullnameCorpus);
}

GBENCHMARK(fullFixedPath_wildmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(
      state, "README", fullnameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(fullFixedPath_re2)(benchmark::State& state) {
  runBenchmark<RE2Impl>(state, "README", fullnameCorpus);
}

GBENCHMARK(fullFixedPath_re2_case_insensitive)(benchmark::State& state) {
  runBenchmark<RE2Impl>(
      state, "README", fullnameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(fullFixedPath_fixed)(benchmark::State& state) {
  runBenchmark<FixedStringImpl>(state, "README", fullnameCorpus);
}

GBENCHMARK(endswith_globmatch)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(state, "*.txt", basenameCorpus);
}

GBENCHMARK(endswith_globmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(
      state, "*.txt", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(endswith_wildmatch)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(state, "*.txt", basenameCorpus);
}

GBENCHMARK(endswith_wildmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(
      state, "*.txt", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(endswith_re2)(benchmark::State& state) {
  runBenchmark<RE2Impl>(state, "[^/]*\\.txt", basenameCorpus);
}

GBENCHMARK(endswith_re2_case_insensitive)(benchmark::State& state) {
  runBenchmark<RE2Impl>(
      state, "[^/]*\\.txt", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(endswith_fixed)(benchmark::State& state) {
  runBenchmark<EndsWithImpl>(state, ".txt", basenameCorpus);
}

GBENCHMARK(basenameGlob_globmatch)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(state, ".*.swp", basenameCorpus);
}

GBENCHMARK(basenameGlob_globmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(
      state, ".*.swp", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(basenameGlob_wildmatch)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(state, ".*.swp", basenameCorpus);
}

GBENCHMARK(basenameGlob_wildmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(
      state, ".*.swp", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(basenameGlob_re2)(benchmark::State& state) {
  runBenchmark<RE2Impl>(state, "\\.[^/]*\\.swp", basenameCorpus);
}

GBENCHMARK(basenameGlob_re2_case_insensitive)(benchmark::State& state) {
  runBenchmark<RE2Impl>(
      state, "\\.[^/]*\\.swp", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(basenameGlob2_globmatch)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(state, ".*.sw?", basenameCorpus);
}

GBENCHMARK(basenameGlob2_globmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(
      state, ".*.sw?", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(basenameGlob2_wildmatch)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(state, ".*.sw?", basenameCorpus);
}

GBENCHMARK(basenameGlob2_wildmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(
      state, ".*.sw?", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(basenameGlob2_re2)(benchmark::State& state) {
  runBenchmark<RE2Impl>(state, "\\.[^/]*\\.sw[^/]", basenameCorpus);
}

GBENCHMARK(basenameGlob2_re2_case_insensitive)(benchmark::State& state) {
  runBenchmark<RE2Impl>(
      state, "\\.[^/]*\\.sw[^/]", basenameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(fullpath_globmatch)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(state, "**/*io*o*", fullnameCorpus);
}

GBENCHMARK(fullpath_globmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<GlobMatcherImpl>(
      state, "**/*io*o*", fullnameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(fullpath_wildmatch)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(state, "**/*io*o*", fullnameCorpus);
}

GBENCHMARK(fullpath_wildmatch_case_insensitive)(benchmark::State& state) {
  runBenchmark<WildmatchImpl>(
      state, "**/*io*o*", fullnameCorpus, CaseSensitivity::Insensitive);
}

GBENCHMARK(fullpath_re2)(benchmark::State& state) {
  runBenchmark<RE2Impl>(state, ".*/[^/]io[^/]*o[^/]*", fullnameCorpus);
}

GBENCHMARK(fullpath_re2_case_insensitive)(benchmark::State& state) {
  runBenchmark<RE2Impl>(
      state,
      ".*/[^/]io[^/]*o[^/]*",
      fullnameCorpus,
      CaseSensitivity::Insensitive);
}

BENCHMARK_MAIN();
