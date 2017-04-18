/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

void ztopy_compression_parameters(CompressionParametersObject* params, ZSTD_compressionParameters* zparams) {
	zparams->windowLog = params->windowLog;
	zparams->chainLog = params->chainLog;
	zparams->hashLog = params->hashLog;
	zparams->searchLog = params->searchLog;
	zparams->searchLength = params->searchLength;
	zparams->targetLength = params->targetLength;
	zparams->strategy = params->strategy;
}

CompressionParametersObject* get_compression_parameters(PyObject* self, PyObject* args) {
	int compressionLevel;
	unsigned PY_LONG_LONG sourceSize = 0;
	Py_ssize_t dictSize = 0;
	ZSTD_compressionParameters params;
	CompressionParametersObject* result;

	if (!PyArg_ParseTuple(args, "i|Kn:get_compression_parameters",
		&compressionLevel, &sourceSize, &dictSize)) {
		return NULL;
	}

	params = ZSTD_getCParams(compressionLevel, sourceSize, dictSize);

	result = PyObject_New(CompressionParametersObject, &CompressionParametersType);
	if (!result) {
		return NULL;
	}

	result->windowLog = params.windowLog;
	result->chainLog = params.chainLog;
	result->hashLog = params.hashLog;
	result->searchLog = params.searchLog;
	result->searchLength = params.searchLength;
	result->targetLength = params.targetLength;
	result->strategy = params.strategy;

	return result;
}

static int CompressionParameters_init(CompressionParametersObject* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"window_log",
		"chain_log",
		"hash_log",
		"search_log",
		"search_length",
		"target_length",
		"strategy",
		NULL
	};

	unsigned windowLog;
	unsigned chainLog;
	unsigned hashLog;
	unsigned searchLog;
	unsigned searchLength;
	unsigned targetLength;
	unsigned strategy;
	ZSTD_compressionParameters params;
	size_t zresult;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "IIIIIII:CompressionParameters",
		kwlist, &windowLog, &chainLog, &hashLog, &searchLog, &searchLength,
		&targetLength, &strategy)) {
		return -1;
	}

	if (windowLog < ZSTD_WINDOWLOG_MIN || windowLog > ZSTD_WINDOWLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid window log value");
		return -1;
	}

	if (chainLog < ZSTD_CHAINLOG_MIN || chainLog > ZSTD_CHAINLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid chain log value");
		return -1;
	}

	if (hashLog < ZSTD_HASHLOG_MIN || hashLog > ZSTD_HASHLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid hash log value");
		return -1;
	}

	if (searchLog < ZSTD_SEARCHLOG_MIN || searchLog > ZSTD_SEARCHLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid search log value");
		return -1;
	}

	if (searchLength < ZSTD_SEARCHLENGTH_MIN || searchLength > ZSTD_SEARCHLENGTH_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid search length value");
		return -1;
	}

	if (targetLength < ZSTD_TARGETLENGTH_MIN || targetLength > ZSTD_TARGETLENGTH_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid target length value");
		return -1;
	}

	if (strategy < ZSTD_fast || strategy > ZSTD_btopt) {
		PyErr_SetString(PyExc_ValueError, "invalid strategy value");
		return -1;
	}

	self->windowLog = windowLog;
	self->chainLog = chainLog;
	self->hashLog = hashLog;
	self->searchLog = searchLog;
	self->searchLength = searchLength;
	self->targetLength = targetLength;
	self->strategy = strategy;

	ztopy_compression_parameters(self, &params);
	zresult = ZSTD_checkCParams(params);

	if (ZSTD_isError(zresult)) {
		PyErr_Format(PyExc_ValueError, "invalid compression parameters: %s",
			ZSTD_getErrorName(zresult));
		return -1;
	}

	return 0;
}

PyDoc_STRVAR(CompressionParameters_estimated_compression_context_size__doc__,
"Estimate the size in bytes of a compression context for compression parameters\n"
);

PyObject* CompressionParameters_estimated_compression_context_size(CompressionParametersObject* self) {
	ZSTD_compressionParameters params;

	ztopy_compression_parameters(self, &params);

	return PyLong_FromSize_t(ZSTD_estimateCCtxSize(params));
}

PyObject* estimate_compression_context_size(PyObject* self, PyObject* args) {
	CompressionParametersObject* params;
	ZSTD_compressionParameters zparams;
	PyObject* result;

	if (!PyArg_ParseTuple(args, "O!:estimate_compression_context_size",
		&CompressionParametersType, &params)) {
		return NULL;
	}

	ztopy_compression_parameters(params, &zparams);
	result = PyLong_FromSize_t(ZSTD_estimateCCtxSize(zparams));
	return result;
}

PyDoc_STRVAR(CompressionParameters__doc__,
"CompressionParameters: low-level control over zstd compression");

static void CompressionParameters_dealloc(PyObject* self) {
	PyObject_Del(self);
}

static PyMethodDef CompressionParameters_methods[] = {
	{
		"estimated_compression_context_size",
		(PyCFunction)CompressionParameters_estimated_compression_context_size,
		METH_NOARGS,
		CompressionParameters_estimated_compression_context_size__doc__
	},
	{ NULL, NULL }
};

static PyMemberDef CompressionParameters_members[] = {
	{ "window_log", T_UINT,
	  offsetof(CompressionParametersObject, windowLog), READONLY,
	  "window log" },
	{ "chain_log", T_UINT,
	  offsetof(CompressionParametersObject, chainLog), READONLY,
	  "chain log" },
	{ "hash_log", T_UINT,
	  offsetof(CompressionParametersObject, hashLog), READONLY,
	  "hash log" },
	{ "search_log", T_UINT,
	  offsetof(CompressionParametersObject, searchLog), READONLY,
	  "search log" },
	{ "search_length", T_UINT,
	  offsetof(CompressionParametersObject, searchLength), READONLY,
	  "search length" },
	{ "target_length", T_UINT,
	  offsetof(CompressionParametersObject, targetLength), READONLY,
	  "target length" },
	{ "strategy", T_INT,
	  offsetof(CompressionParametersObject, strategy), READONLY,
	  "strategy" },
	{ NULL }
};

PyTypeObject CompressionParametersType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"CompressionParameters", /* tp_name */
	sizeof(CompressionParametersObject), /* tp_basicsize */
	0,                         /* tp_itemsize */
	(destructor)CompressionParameters_dealloc, /* tp_dealloc */
	0,                         /* tp_print */
	0,                         /* tp_getattr */
	0,                         /* tp_setattr */
	0,                         /* tp_compare */
	0,                         /* tp_repr */
	0,                         /* tp_as_number */
	0,                         /* tp_as_sequence */
	0,                         /* tp_as_mapping */
	0,                         /* tp_hash  */
	0,                         /* tp_call */
	0,                         /* tp_str */
	0,                         /* tp_getattro */
	0,                         /* tp_setattro */
	0,                         /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT | Py_TPFLAGS_BASETYPE, /* tp_flags */
	CompressionParameters__doc__, /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	CompressionParameters_methods, /* tp_methods */
	CompressionParameters_members, /* tp_members */
	0,                         /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	(initproc)CompressionParameters_init, /* tp_init */
	0,                         /* tp_alloc */
	PyType_GenericNew,         /* tp_new */
};

void compressionparams_module_init(PyObject* mod) {
	Py_TYPE(&CompressionParametersType) = &PyType_Type;
	if (PyType_Ready(&CompressionParametersType) < 0) {
		return;
	}

	Py_INCREF(&CompressionParametersType);
	PyModule_AddObject(mod, "CompressionParameters",
		(PyObject*)&CompressionParametersType);
}
