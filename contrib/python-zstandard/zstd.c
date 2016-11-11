/**
 * Copyright (c) 2016-present, Gregory Szorc
 * All rights reserved.
 *
 * This software may be modified and distributed under the terms
 * of the BSD license. See the LICENSE file for details.
 */

/* A Python C extension for Zstandard. */

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

PyDoc_STRVAR(train_dictionary__doc__,
"train_dictionary(dict_size, samples)\n"
"\n"
"Train a dictionary from sample data.\n"
"\n"
"A compression dictionary of size ``dict_size`` will be created from the\n"
"iterable of samples provided by ``samples``.\n"
"\n"
"The raw dictionary content will be returned\n");

static char zstd_doc[] = "Interface to zstandard";

static PyMethodDef zstd_methods[] = {
	{ "estimate_compression_context_size", (PyCFunction)estimate_compression_context_size,
	METH_VARARGS, estimate_compression_context_size__doc__ },
	{ "estimate_decompression_context_size", (PyCFunction)estimate_decompression_context_size,
	METH_NOARGS, estimate_decompression_context_size__doc__ },
	{ "get_compression_parameters", (PyCFunction)get_compression_parameters,
	METH_VARARGS, get_compression_parameters__doc__ },
	{ "train_dictionary", (PyCFunction)train_dictionary,
	METH_VARARGS | METH_KEYWORDS, train_dictionary__doc__ },
	{ NULL, NULL }
};

void compressobj_module_init(PyObject* mod);
void compressor_module_init(PyObject* mod);
void compressionparams_module_init(PyObject* mod);
void constants_module_init(PyObject* mod);
void dictparams_module_init(PyObject* mod);
void compressiondict_module_init(PyObject* mod);
void compressionwriter_module_init(PyObject* mod);
void compressoriterator_module_init(PyObject* mod);
void decompressor_module_init(PyObject* mod);
void decompressobj_module_init(PyObject* mod);
void decompressionwriter_module_init(PyObject* mod);
void decompressoriterator_module_init(PyObject* mod);

void zstd_module_init(PyObject* m) {
	compressionparams_module_init(m);
	dictparams_module_init(m);
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
