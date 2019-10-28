/* traceprofimpl.cpp - main implementation of traceprofile
 *
 * Copyright 2017 Facebook, Inc.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version. */
#include "Python.h"
#include "frameobject.h"

#if PY_MAJOR_VERSION >= 3
#define IS_PY3K
#endif

#include <algorithm>
#include <cassert>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <list>
#include <string>
#include <unordered_map>
#include <vector>

#ifndef _WIN32
#include <sys/time.h>
#include <unistd.h>
#endif

typedef int lineno_t;
typedef uint64_t frameid_t;
typedef uint64_t rdtsc_t;

/* tracing ------------------------------------------------------------------ */

/* information about a raw Python frame */
struct FrameInfo {
  PyCodeObject* code; /* PyCodeObject */
  frameid_t back;

  /* needed by older C++ map which has difficulty on zero-copy assignment
   */
  FrameInfo() {
    code = NULL;
    back = 0;
  }

  void assign(PyFrameObject* frame, frameid_t backfid) {
    back = backfid;
    code = frame->f_code;
    Py_XINCREF(code);
  }

  lineno_t line() {
    if (!code)
      return 0;
    return code->co_firstlineno;
  }

  const char* file() {
    if (!code)
      return NULL;
#ifdef IS_PY3K
    PyObject* obj =
        PyUnicode_AsEncodedString(code->co_filename, "utf-8", "strict");
    if (!obj)
      return NULL;
    return PyBytes_AsString(obj);
#else
    return PyString_AsString(code->co_filename);
#endif
  }

  const char* name() {
    if (!code)
      return NULL;
#ifdef IS_PY3K
    PyObject* obj = PyUnicode_AsEncodedString(code->co_name, "utf-8", "strict");
    if (!obj)
      return NULL;
    return PyBytes_AsString(obj);
#else
    return PyString_AsString(code->co_name);
#endif
  }

  ~FrameInfo() {
    Py_XDECREF(code);
    code = NULL;
  }

 private:
  /* forbid copy */
  FrameInfo(const FrameInfo& rhs);
  FrameInfo(const FrameInfo&& rhs);
};

/* samples */
struct Sample {
  rdtsc_t time;
  frameid_t frameid;
  int op; /* PyTrace_{CALL,EXCEPTION,LINE,RETURN,C_CALL,C_EXCEPTION,C_RETURN}
           */
};

/* global state, collected frames and samples */
static std::unordered_map<frameid_t, FrameInfo> frames;
static std::list<Sample> samples;

/* for measuring wall time / rdtsc ratio */
static uint64_t t1, t2;
static rdtsc_t r1, r2;
static double rdtscratio; /* set by disable() */

/* read microseconds using std::chrono */
static uint64_t now_microseconds() {
  using namespace std::chrono;
  auto now = high_resolution_clock::now();
  return duration_cast<microseconds>(now.time_since_epoch()).count();
}

/* fast (best-effort) way to get wall time */
inline static rdtsc_t rdtsc() {
#if defined(__aarch64__) && !defined(_MSC_VER) /* aarch64 fast path */
  unsigned long long val;
  asm volatile("mrs %0, cntvct_el0" : "=r"(val));
  return val;
#elif defined(__amd64__) && !defined(_MSC_VER) /* x64 fast path */
  unsigned long lo, hi;
  asm volatile("rdtsc" : "=a"(lo), "=d"(hi));
  return (rdtsc_t)(lo | (hi << 32));
#else /* other platform or MSVC (no inline asm support) */
  return (rdtsc_t)(now_microseconds());
#endif
}

/* fast, but inaccurate hashing of a Python frame */
inline static uint64_t hashframe(PyFrameObject* frame) {
  uint64_t v =
      ((uint64_t)frame ^ ((uint64_t)(frame->f_back) << 16) ^
       ((uint64_t)frame->f_code << 32));
  /* f_code is usually immutable (lsprof use its address as keys) */
  return v;
}

/* hash, and store a Python frame, return its ID */
static frameid_t hashandstoreframe(PyFrameObject* frame) {
  if (!frame)
    return 0;
  frameid_t frameid = (frameid_t)hashframe(frame);
  if (frames.count(frameid) == 0) {
    frames[frameid].assign(frame, hashandstoreframe(frame->f_back));
  }
  return frameid;
}

/* record a sample */
inline static void recordframe(PyFrameObject* frame, int op) {
  frameid_t fid = hashandstoreframe(frame);
  samples.push_back({rdtsc(), fid, op});
}

static int tracefunc(PyObject* o, PyFrameObject* frame, int op, PyObject* a) {
  (void)o;
  (void)a;
  recordframe(frame, op);
  return 0;
}

static void enable() {
  r1 = rdtsc();
  t1 = now_microseconds() / 1000;
  PyEval_SetProfile((Py_tracefunc)tracefunc, NULL);
}

static void disable() {
  PyEval_SetProfile(NULL, NULL);
  r2 = rdtsc();
  t2 = now_microseconds() / 1000;
  /* calculate rdtscratio */
  rdtscratio = (t2 - t1) / (double)(r2 - r1);
}

/* reporting ---------------------------------------------------------------- */

struct FrameSummary {
  rdtsc_t time;
  unsigned int count;
};

static std::unordered_map<frameid_t, FrameSummary> summaries;
static std::unordered_map<frameid_t, std::list<frameid_t>> framechildren;

/* for dedup */
static std::unordered_map<frameid_t, uint64_t> fid2hash;
static std::unordered_map<uint64_t, frameid_t> hash2fid;

/* hash FrameInfo, do not be affected by frame addresses */
static uint64_t hashframeinfo(frameid_t fid) {
  if (fid == 0) {
    return 0;
  }
  if (!fid2hash.count(fid)) {
    auto& fi = frames[fid];
    uint64_t v = (uint64_t)fi.code;
    v ^= hashframeinfo(fi.back) << 1;
    fid2hash[fid] = v;
  }
  return fid2hash[fid];
}

/* fill hash2fid */
static void buildframededup() {
  for (auto& s : samples) {
    frameid_t fid = s.frameid;
    while (fid) {
      uint64_t v = hashframeinfo(fid);
      if (hash2fid.count(v) == 0) {
        hash2fid[v] = fid;
        fid = frames[fid].back;
      } else {
        break;
      }
    }
  }
}

static frameid_t dedupfid(frameid_t fid) {
  if (fid2hash.count(fid) == 0) {
    return fid; /* no information available */
  } else {
    return hash2fid[fid2hash[fid]];
  }
}

/* fill calltimes and summaries */
static void buildsummaries() {
  std::unordered_map<frameid_t, std::list<Sample*>> calls;

  for (auto& s : samples) {
    frameid_t fid = dedupfid(s.frameid);
    if (s.op == PyTrace_CALL) {
      calls[fid].push_back(&s);
    } else if (s.op == PyTrace_RETURN) {
      auto& entries = calls[fid];
      if (entries.empty())
        continue;
      /* frame was entered before */
      Sample* prev = entries.back();
      entries.pop_back();
      auto& sum = summaries[fid];
      sum.count += 1;
      if (entries.empty())
        sum.time += s.time - prev->time;
    } /* s.op */
  }
}

/* fill framechildren */
static void buildframetree() {
  for (auto& s : samples) {
    if (s.op != PyTrace_CALL && s.op != PyTrace_C_CALL) {
      continue; /* only interested in call */
    }
    for (frameid_t fid = s.frameid; fid;) {
      fid = dedupfid(fid);
      auto& fi = frames[fid];
      frameid_t pfid = dedupfid(fi.back);
      auto& children = framechildren[pfid];
      int existed = 0;
      for (auto& c : children) {
        if (c == fid) {
          existed = 1;
          break;
        }
      }
      if (existed) {
        break;
      }
      children.push_back(fid);
      fid = pfid;
    } /* for fid */
  }
}

static std::string shortname(std::string path) {
  size_t p = path.rfind('/');
  if (p == std::string::npos)
    return path;
  /* special handling __init__.py, include its dirname */
  if (p > 0 && path.substr(p + 1, p + 1 + 11) == "__init__.py") {
    p = path.rfind('/', p - 1);
    if (p == std::string::npos)
      return path;
  }
  return path.substr(p + 1);
}

/* width needed to output time (in ms) */
static int timelen() {
  static int n = 0;
  if (n)
    return n;
  n = 1;
  rdtsc_t maxframetime = 0;
  for (auto& s : summaries) {
    if (s.second.time > maxframetime)
      maxframetime = s.second.time;
  }
  for (double t = (double)maxframetime * rdtscratio; t >= 10; t /= 10) {
    n++;
  }
  return n;
}

/* printf format to output time */
static const char* timefmt() {
  static char fmt[20] = {0};
  if (fmt[0] == 0) {
    int n = timelen();
    snprintf(fmt, sizeof fmt, "%%%d.0f", n);
  }
  return fmt;
}

static inline int fprintindent(FILE* fp, int indent) {
  for (int i = 0; i < indent; ++i)
    fputc(' ', fp);
  return (indent > 0 ? indent : 0);
}

/* config items */
static double timethreshold = 2;
static size_t countthreshold = 2;
static int dedup = 1;

static void settimethreshold(double ms) {
  timethreshold = ms;
}

static void setcountthreshold(size_t count) {
  countthreshold = count;
}

static void setdedup(int value) {
  dedup = value;
}

static void fprintframetree(
    FILE* fp = stderr,
    frameid_t fid = 0,
    int indent = 0,
    char ch = '|') {
  auto& f = frames[fid];
  auto& s = summaries[fid];

  /* collect (> 2ms) child frames to print */
  std::vector<frameid_t> cfids;
  rdtsc_t ctotaltime = 0;
  for (auto& cfid : framechildren[fid]) {
    auto& cs = summaries[cfid];
    if ((double)cs.time * rdtscratio >= timethreshold || cs.count == 0)
      cfids.push_back(cfid);
    ctotaltime += cs.time;
  }

  /* hot frame? frame time > 2 * sum(child frame time) and frame time >
   * 30ms */
  int hot = (s.time > ctotaltime * 2 && (double)s.time * rdtscratio > 30);

  if (fid != 0) {
    int ncol = 0;

    /* hot symbol */
    if (hot) {
      ncol += fprintf(fp, "* ");
    } else {
      ncol += fprintf(fp, "  ");
    }

    /* time in ms */
    if (s.count > 0) {
      ncol += fprintf(fp, timefmt(), (double)s.time * rdtscratio);
    } else {
      ncol += fprintindent(fp, timelen()); /* call not recorded */
    }

    /* symbol and indent */
    ncol += fprintindent(fp, indent + 1);
    ncol += fprintf(fp, "%c ", ch);

    /* frame name */
    ncol += fprintf(fp, "%s ", f.name());

    /* call count */
    if (s.count >= countthreshold) {
      ncol += fprintf(fp, "(%d times) ", s.count);
    }

    /* file path */
    fprintindent(fp, 48 - ncol);
    std::string path = f.file();
    ncol += fprintf(fp, "%s:%d", shortname(path).c_str(), f.line());

    /* end of line */
    fprintf(fp, "\n");
  }

  /* children */
  indent += (ch == '\\');
  if (cfids.size() > 1) {
    indent += 1;
    ch = '\\';
  } else {
    ch = '|';
  }
  for (auto& cfid : cfids) {
    fprintframetree(fp, cfid, indent, ch);
  }
}

static void clear() {
  summaries.clear();
  framechildren.clear();
  fid2hash.clear();
  hash2fid.clear();
  samples.clear();
  frames.clear();
}

static void report(FILE* fp = stderr) {
  if (dedup)
    buildframededup();
  buildsummaries();
  buildframetree();
  fprintframetree(fp, dedupfid(0));
  fprintf(fp, "Total time: %.0f ms\n", (double)(r2 - r1) * rdtscratio);
}
