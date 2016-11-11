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

	if (!PyArg_ParseTuple(args, "i|Kn", &compressionLevel, &sourceSize, &dictSize)) {
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

PyObject* estimate_compression_context_size(PyObject* self, PyObject* args) {
	CompressionParametersObject* params;
	ZSTD_compressionParameters zparams;
	PyObject* result;

	if (!PyArg_ParseTuple(args, "O!", &CompressionParametersType, &params)) {
		return NULL;
	}

	ztopy_compression_parameters(params, &zparams);
	result = PyLong_FromSize_t(ZSTD_estimateCCtxSize(zparams));
	return result;
}

PyDoc_STRVAR(CompressionParameters__doc__,
"CompressionParameters: low-level control over zstd compression");

static PyObject* CompressionParameters_new(PyTypeObject* subtype, PyObject* args, PyObject* kwargs) {
	CompressionParametersObject* self;
	unsigned windowLog;
	unsigned chainLog;
	unsigned hashLog;
	unsigned searchLog;
	unsigned searchLength;
	unsigned targetLength;
	unsigned strategy;

	if (!PyArg_ParseTuple(args, "IIIIIII", &windowLog, &chainLog, &hashLog, &searchLog,
		&searchLength, &targetLength, &strategy)) {
		return NULL;
	}

	if (windowLog < ZSTD_WINDOWLOG_MIN || windowLog > ZSTD_WINDOWLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid window log value");
		return NULL;
	}

	if (chainLog < ZSTD_CHAINLOG_MIN || chainLog > ZSTD_CHAINLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid chain log value");
		return NULL;
	}

	if (hashLog < ZSTD_HASHLOG_MIN || hashLog > ZSTD_HASHLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid hash log value");
		return NULL;
	}

	if (searchLog < ZSTD_SEARCHLOG_MIN || searchLog > ZSTD_SEARCHLOG_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid search log value");
		return NULL;
	}

	if (searchLength < ZSTD_SEARCHLENGTH_MIN || searchLength > ZSTD_SEARCHLENGTH_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid search length value");
		return NULL;
	}

	if (targetLength < ZSTD_TARGETLENGTH_MIN || targetLength > ZSTD_TARGETLENGTH_MAX) {
		PyErr_SetString(PyExc_ValueError, "invalid target length value");
		return NULL;
	}

	if (strategy < ZSTD_fast || strategy > ZSTD_btopt) {
		PyErr_SetString(PyExc_ValueError, "invalid strategy value");
		return NULL;
	}

	self = (CompressionParametersObject*)subtype->tp_alloc(subtype, 1);
	if (!self) {
		return NULL;
	}

	self->windowLog = windowLog;
	self->chainLog = chainLog;
	self->hashLog = hashLog;
	self->searchLog = searchLog;
	self->searchLength = searchLength;
	self->targetLength = targetLength;
	self->strategy = strategy;

	return (PyObject*)self;
}

static void CompressionParameters_dealloc(PyObject* self) {
	PyObject_Del(self);
}

static Py_ssize_t CompressionParameters_length(PyObject* self) {
	return 7;
};

static PyObject* CompressionParameters_item(PyObject* o, Py_ssize_t i) {
	CompressionParametersObject* self = (CompressionParametersObject*)o;

	switch (i) {
	case 0:
		return PyLong_FromLong(self->windowLog);
	case 1:
		return PyLong_FromLong(self->chainLog);
	case 2:
		return PyLong_FromLong(self->hashLog);
	case 3:
		return PyLong_FromLong(self->searchLog);
	case 4:
		return PyLong_FromLong(self->searchLength);
	case 5:
		return PyLong_FromLong(self->targetLength);
	case 6:
		return PyLong_FromLong(self->strategy);
	default:
		PyErr_SetString(PyExc_IndexError, "index out of range");
		return NULL;
	}
}

static PySequenceMethods CompressionParameters_sq = {
	CompressionParameters_length, /* sq_length */
	0,							  /* sq_concat */
	0,                            /* sq_repeat */
	CompressionParameters_item,   /* sq_item */
	0,                            /* sq_ass_item */
	0,                            /* sq_contains */
	0,                            /* sq_inplace_concat */
	0                             /* sq_inplace_repeat */
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
	&CompressionParameters_sq, /* tp_as_sequence */
	0,                         /* tp_as_mapping */
	0,                         /* tp_hash  */
	0,                         /* tp_call */
	0,                         /* tp_str */
	0,                         /* tp_getattro */
	0,                         /* tp_setattro */
	0,                         /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT,        /* tp_flags */
	CompressionParameters__doc__, /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	0,                         /* tp_methods */
	0,                         /* tp_members */
	0,                         /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	0,                         /* tp_init */
	0,                         /* tp_alloc */
	CompressionParameters_new, /* tp_new */
};

void compressionparams_module_init(PyObject* mod) {
	Py_TYPE(&CompressionParametersType) = &PyType_Type;
	if (PyType_Ready(&CompressionParametersType) < 0) {
		return;
	}

	Py_IncRef((PyObject*)&CompressionParametersType);
	PyModule_AddObject(mod, "CompressionParameters",
		(PyObject*)&CompressionParametersType);
}
