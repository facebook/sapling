/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

PyDoc_STRVAR(DictParameters__doc__,
"DictParameters: low-level control over dictionary generation");

static PyObject* DictParameters_new(PyTypeObject* subtype, PyObject* args, PyObject* kwargs) {
	DictParametersObject* self;
	unsigned selectivityLevel;
	int compressionLevel;
	unsigned notificationLevel;
	unsigned dictID;

	if (!PyArg_ParseTuple(args, "IiII:DictParameters",
		&selectivityLevel, &compressionLevel, &notificationLevel, &dictID)) {
		return NULL;
	}

	self = (DictParametersObject*)subtype->tp_alloc(subtype, 1);
	if (!self) {
		return NULL;
	}

	self->selectivityLevel = selectivityLevel;
	self->compressionLevel = compressionLevel;
	self->notificationLevel = notificationLevel;
	self->dictID = dictID;

	return (PyObject*)self;
}

static void DictParameters_dealloc(PyObject* self) {
	PyObject_Del(self);
}

static PyMemberDef DictParameters_members[] = {
	{ "selectivity_level", T_UINT,
	  offsetof(DictParametersObject, selectivityLevel), READONLY,
	  "selectivity level" },
	{ "compression_level", T_INT,
	  offsetof(DictParametersObject, compressionLevel), READONLY,
	  "compression level" },
	{ "notification_level", T_UINT,
	  offsetof(DictParametersObject, notificationLevel), READONLY,
	  "notification level" },
	{ "dict_id", T_UINT,
	  offsetof(DictParametersObject, dictID), READONLY,
	  "dictionary ID" },
	{ NULL }
};

static Py_ssize_t DictParameters_length(PyObject* self) {
	return 4;
}

static PyObject* DictParameters_item(PyObject* o, Py_ssize_t i) {
	DictParametersObject* self = (DictParametersObject*)o;

	switch (i) {
	case 0:
		return PyLong_FromLong(self->selectivityLevel);
	case 1:
		return PyLong_FromLong(self->compressionLevel);
	case 2:
		return PyLong_FromLong(self->notificationLevel);
	case 3:
		return PyLong_FromLong(self->dictID);
	default:
		PyErr_SetString(PyExc_IndexError, "index out of range");
		return NULL;
	}
}

static PySequenceMethods DictParameters_sq = {
	DictParameters_length, /* sq_length */
	0,	                   /* sq_concat */
	0,                     /* sq_repeat */
	DictParameters_item,   /* sq_item */
	0,                     /* sq_ass_item */
	0,                     /* sq_contains */
	0,                     /* sq_inplace_concat */
	0                      /* sq_inplace_repeat */
};

PyTypeObject DictParametersType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"DictParameters", /* tp_name */
	sizeof(DictParametersObject), /* tp_basicsize */
	0,                         /* tp_itemsize */
	(destructor)DictParameters_dealloc, /* tp_dealloc */
	0,                         /* tp_print */
	0,                         /* tp_getattr */
	0,                         /* tp_setattr */
	0,                         /* tp_compare */
	0,                         /* tp_repr */
	0,                         /* tp_as_number */
	&DictParameters_sq,        /* tp_as_sequence */
	0,                         /* tp_as_mapping */
	0,                         /* tp_hash  */
	0,                         /* tp_call */
	0,                         /* tp_str */
	0,                         /* tp_getattro */
	0,                         /* tp_setattro */
	0,                         /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT,        /* tp_flags */
	DictParameters__doc__,     /* tp_doc */
	0,                         /* tp_traverse */
	0,                         /* tp_clear */
	0,                         /* tp_richcompare */
	0,                         /* tp_weaklistoffset */
	0,                         /* tp_iter */
	0,                         /* tp_iternext */
	0,                         /* tp_methods */
	DictParameters_members,    /* tp_members */
	0,                         /* tp_getset */
	0,                         /* tp_base */
	0,                         /* tp_dict */
	0,                         /* tp_descr_get */
	0,                         /* tp_descr_set */
	0,                         /* tp_dictoffset */
	0,                         /* tp_init */
	0,                         /* tp_alloc */
	DictParameters_new,        /* tp_new */
};

void dictparams_module_init(PyObject* mod) {
	Py_TYPE(&DictParametersType) = &PyType_Type;
	if (PyType_Ready(&DictParametersType) < 0) {
		return;
	}

	Py_IncRef((PyObject*)&DictParametersType);
	PyModule_AddObject(mod, "DictParameters", (PyObject*)&DictParametersType);
}
