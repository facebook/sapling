/* traceprofimpl.cpp - main implementation of traceprofile
 *
 * Copyright 2017 Facebook, Inc.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version. */
#include "Python.h"
#include "frameobject.h"

#include <algorithm>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <list>
#include <string>
#include <unordered_map>
#include <vector>

#include <sys/time.h>
#include <unistd.h>

typedef int lineno_t;
typedef uint64_t frameid_t;
typedef uint64_t rdtsc_t;

/* tracing ------------------------------------------------------------------ */

/* information about a raw Python frame */
struct FrameInfo {
  std::string file;
  std::string name;
  frameid_t back;
  lineno_t line;
};

/* samples */
struct Sample {
  rdtsc_t time;
  frameid_t frameid;
  int op; /* PyTrace_{CALL,EXCEPTION,LINE,RETURN,C_CALL,C_EXCEPTION,C_RETURN} */
};

/* global state, collected frames and samples */
static std::unordered_map<frameid_t, FrameInfo> frames;
static std::list<Sample> samples;

/* for measuring wall time / rdtsc ratio */
static struct timeval t1, t2;
static rdtsc_t r1, r2;
static double rdtscratio; /* set by disable() */

/* fast way to get wall time */
inline static rdtsc_t rdtsc() {
	unsigned long lo, hi;
	asm volatile ("rdtsc" : "=a" (lo), "=d" (hi));
	return (rdtsc_t)(lo | (hi << 32));
}

/* fast, but inaccurate hashing of a Python frame */
inline static uint64_t hashframe(PyFrameObject *frame) {
	uint64_t v = ((uint64_t)frame ^ ((uint64_t)(frame->f_back) << 16) ^
			((uint64_t)frame->f_code << 32));
  /* f_code is usually immutable (lsprof use its address as keys) */
	return v;
}

/* hash, and store a Python frame, return its ID */
static frameid_t hashandstoreframe(PyFrameObject *frame) {
  if (!frame) return 0;
  frameid_t frameid = (frameid_t)hashframe(frame);
  if (frames.count(frameid) == 0) {
    FrameInfo fi;
    fi.file = PyString_AsString(frame->f_code->co_filename);
    fi.name = PyString_AsString(frame->f_code->co_name);
    fi.back = hashandstoreframe(frame->f_back);
    fi.line = frame->f_code->co_firstlineno;
    frames[frameid] = fi;
  }
  return frameid;
}

/* record a sample */
inline static void recordframe(PyFrameObject *frame, int op) {
  frameid_t fid = hashandstoreframe(frame);
  samples.push_back({rdtsc(), fid, op});
}

static int tracefunc(PyObject *o, PyFrameObject *frame, int op, PyObject *a) {
  (void)o;
  (void)a;
  recordframe(frame, op);
  return 0;
}

static void enable() {
  r1 = rdtsc();
  gettimeofday(&t1, NULL);
  PyEval_SetProfile((Py_tracefunc) tracefunc, NULL);
}

static void disable() {
  PyEval_SetProfile(NULL, NULL);
  r2 = rdtsc();
  gettimeofday(&t2, NULL);
  /* calculate rdtscratio */
  double dt = 0; /* ms */
  dt += (double)(t2.tv_sec - t1.tv_sec) * 1000.0;  /* sec to ms */
  dt += (double)(t2.tv_usec - t1.tv_usec) / 1000.0;  /* us to ms */
  rdtscratio = dt / (double)(r2 - r1);
}

/* reporting ---------------------------------------------------------------- */

struct FrameSummary {
  rdtsc_t time;
  unsigned int count;
};

static std::unordered_map<frameid_t, FrameSummary> summaries;
static std::unordered_map<frameid_t, std::list<frameid_t> > framechildren;

/* fill calltimes and summaries */
static void buildsummaries() {
  std::unordered_map<frameid_t, std::list<Sample*>> calls;

  for (auto& s : samples) {
    frameid_t fid = s.frameid;
    if (s.op == PyTrace_CALL) {
      calls[fid].push_back(&s);
    } else if (s.op == PyTrace_RETURN) {
      auto& entries = calls[fid];
      if (entries.empty())
        continue;
      /* frame was entered before */
      Sample* prev = entries.back();
      entries.pop_back();
      auto &sum = summaries[fid];
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
      auto& fi = frames[fid];
      frameid_t pfid = fi.back;
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
    n ++;
  }
  return n;
}

/* printf format to output time */
static const char *timefmt() {
  static char fmt[20] = { 0 };
  if (fmt[0] == 0) {
    int n = timelen();
    snprintf(fmt, sizeof fmt, "%%%d.0f", n);
  }
  return fmt;
}

static inline int fprintindent(FILE *fp, int indent) {
  for (int i = 0; i < indent; ++i) fputc(' ', fp);
  return (indent > 0 ? indent : 0);
}

/* config items */
static double timethreshold = 2;
static size_t countthreshold = 2;

static void settimethreshold(double ms) {
  timethreshold = ms;
}

static void setcountthreshold(size_t count) {
  countthreshold = count;
}

static void fprintframetree(FILE *fp = stderr, frameid_t fid = 0,
    int indent = 0, char ch = '|') {
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

  /* hot frame? frame time > 2 * sum(child frame time) and frame time > 30ms */
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
    ncol += fprintf(fp, "%s ", f.name.c_str());

    /* call count */
    if (s.count >= countthreshold) {
      ncol += fprintf(fp, "(%d times) ", s.count);
    }

    /* file path */
    fprintindent(fp, 48 - ncol);
    std::string path = f.file;
    ncol += fprintf(fp, "%s:%d", shortname(path).c_str(), f.line);

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

static void report(FILE *fp = stderr) {
  buildsummaries();
  buildframetree();
  fprintframetree(fp);
  fprintf(fp, "Total time: %.0f ms\n", (double)(r2 - r1) * rdtscratio);
}
