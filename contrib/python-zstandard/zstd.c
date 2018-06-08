/**
 * Copyright (c) 2016-present, Gregory Szorc
 * All rights reserved.
 *
 * This software may be modified and distributed under the terms
 * of the BSD license. See the LICENSE file for details.
 */

/* A Python C extension for Zstandard. */

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#elif defined(__APPLE__) || defined(__OpenBSD__) || defined(__FreeBSD__) || defined(__NetBSD__) || defined(__DragonFly__)
#include <sys/types.h>
#include <sys/sysctl.h>
#endif

#include "python-zstandard.h"

PyObject *ZstdError;

PyDoc_STRVAR(estimate_compression_context_size__doc__,
"estimate_compression_context_size(compression_parameters)\n"
"\n"
"Give the amount of memory allocated for a compression context given a\n"
"CompressionParameters instance");

PyDoc_STRVAR(estimate_decompression_context_size__doc__,
"estimate_decompression_context_size()\n"
"\n"
"Estimate the amount of memory allocated to a decompression context.\n"
);

static PyObject* estimate_decompression_context_size(PyObject* self) {
	return PyLong_FromSize_t(ZSTD_estimateDCtxSize());
}

PyDoc_STRVAR(get_compression_parameters__doc__,
"get_compression_parameters(compression_level[, source_size[, dict_size]])\n"
"\n"
"Obtains a ``CompressionParameters`` instance from a compression level and\n"
"optional input size and dictionary size");

PyDoc_STRVAR(get_frame_parameters__doc__,
"get_frame_parameters(data)\n"
"\n"
"Obtains a ``FrameParameters`` instance by parsing data.\n");

PyDoc_STRVAR(train_dictionary__doc__,
"train_dictionary(dict_size, samples)\n"
"\n"
"Train a dictionary from sample data.\n"
"\n"
"A compression dictionary of size ``dict_size`` will be created from the\n"
"iterable of samples provided by ``samples``.\n"
"\n"
"The raw dictionary content will be returned\n");

PyDoc_STRVAR(train_cover_dictionary__doc__,
"train_cover_dictionary(dict_size, samples, k=None, d=None, notifications=0, dict_id=0, level=0)\n"
"\n"
"Train a dictionary from sample data using the COVER algorithm.\n"
"\n"
"This behaves like ``train_dictionary()`` except a different algorithm is\n"
"used to create the dictionary. The algorithm has 2 parameters: ``k`` and\n"
"``d``. These control the *segment size* and *dmer size*. A reasonable range\n"
"for ``k`` is ``[16, 2048+]``. A reasonable range for ``d`` is ``[6, 16]``.\n"
"``d`` must be less than or equal to ``k``.\n"
);

static char zstd_doc[] = "Interface to zstandard";

static PyMethodDef zstd_methods[] = {
	/* TODO remove since it is a method on CompressionParameters. */
	{ "estimate_compression_context_size", (PyCFunction)estimate_compression_context_size,
	METH_VARARGS, estimate_compression_context_size__doc__ },
	{ "estimate_decompression_context_size", (PyCFunction)estimate_decompression_context_size,
	METH_NOARGS, estimate_decompression_context_size__doc__ },
	{ "get_compression_parameters", (PyCFunction)get_compression_parameters,
	METH_VARARGS, get_compression_parameters__doc__ },
	{ "get_frame_parameters", (PyCFunction)get_frame_parameters,
	METH_VARARGS, get_frame_parameters__doc__ },
	{ "train_dictionary", (PyCFunction)train_dictionary,
	METH_VARARGS | METH_KEYWORDS, train_dictionary__doc__ },
	{ "train_cover_dictionary", (PyCFunction)train_cover_dictionary,
	METH_VARARGS | METH_KEYWORDS, train_cover_dictionary__doc__ },
	{ NULL, NULL }
};

void bufferutil_module_init(PyObject* mod);
void compressobj_module_init(PyObject* mod);
void compressor_module_init(PyObject* mod);
void compressionparams_module_init(PyObject* mod);
void constants_module_init(PyObject* mod);
void compressiondict_module_init(PyObject* mod);
void compressionwriter_module_init(PyObject* mod);
void compressoriterator_module_init(PyObject* mod);
void decompressor_module_init(PyObject* mod);
void decompressobj_module_init(PyObject* mod);
void decompressionwriter_module_init(PyObject* mod);
void decompressoriterator_module_init(PyObject* mod);
void frameparams_module_init(PyObject* mod);

void zstd_module_init(PyObject* m) {
	/* python-zstandard relies on unstable zstd C API features. This means
	   that changes in zstd may break expectations in python-zstandard.

	   python-zstandard is distributed with a copy of the zstd sources.
	   python-zstandard is only guaranteed to work with the bundled version
	   of zstd.

	   However, downstream redistributors or packagers may unbundle zstd
	   from python-zstandard. This can result in a mismatch between zstd
	   versions and API semantics. This essentially "voids the warranty"
	   of python-zstandard and may cause undefined behavior.

	   We detect this mismatch here and refuse to load the module if this
	   scenario is detected.
	*/
	if (ZSTD_VERSION_NUMBER != 10103 || ZSTD_versionNumber() != 10103) {
		PyErr_SetString(PyExc_ImportError, "zstd C API mismatch; Python bindings not compiled against expected zstd version");
		return;
	}

	bufferutil_module_init(m);
	compressionparams_module_init(m);
	compressiondict_module_init(m);
	compressobj_module_init(m);
	compressor_module_init(m);
	compressionwriter_module_init(m);
	compressoriterator_module_init(m);
	constants_module_init(m);
	decompressor_module_init(m);
	decompressobj_module_init(m);
	decompressionwriter_module_init(m);
	decompressoriterator_module_init(m);
	frameparams_module_init(m);
}

#if PY_MAJOR_VERSION >= 3
static struct PyModuleDef zstd_module = {
	PyModuleDef_HEAD_INIT,
	"zstd",
	zstd_doc,
	-1,
	zstd_methods
};

PyMODINIT_FUNC PyInit_zstd(void) {
	PyObject *m = PyModule_Create(&zstd_module);
	if (m) {
		zstd_module_init(m);
		if (PyErr_Occurred()) {
			Py_DECREF(m);
			m = NULL;
		}
	}
	return m;
}
#else
PyMODINIT_FUNC initzstd(void) {
	PyObject *m = Py_InitModule3("zstd", zstd_methods, zstd_doc);
	if (m) {
		zstd_module_init(m);
	}
}
#endif

/* Attempt to resolve the number of CPUs in the system. */
int cpu_count() {
	int count = 0;

#if defined(_WIN32)
	SYSTEM_INFO si;
	si.dwNumberOfProcessors = 0;
	GetSystemInfo(&si);
	count = si.dwNumberOfProcessors;
#elif defined(__APPLE__)
	int num;
	size_t size = sizeof(int);

	if (0 == sysctlbyname("hw.logicalcpu", &num, &size, NULL, 0)) {
		count = num;
	}
#elif defined(__linux__)
	count = sysconf(_SC_NPROCESSORS_ONLN);
#elif defined(__OpenBSD__) || defined(__FreeBSD__) || defined(__NetBSD__) || defined(__DragonFly__)
	int mib[2];
	size_t len = sizeof(count);
	mib[0] = CTL_HW;
	mib[1] = HW_NCPU;
	if (0 != sysctl(mib, 2, &count, &len, NULL, 0)) {
		count = 0;
	}
#elif defined(__hpux)
	count = mpctl(MPC_GETNUMSPUS, NULL, NULL);
#endif

	return count;
}

size_t roundpow2(size_t i) {
	i--;
	i |= i >> 1;
	i |= i >> 2;
	i |= i >> 4;
	i |= i >> 8;
	i |= i >> 16;
	i++;

	return i;
}
