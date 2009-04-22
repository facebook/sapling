/* 
 * Copyright (C) 2009 Jelmer Vernooij <jelmer@samba.org>
 *
 * This program is free software; you can redistribute it and/or
 * modify it under the terms of the GNU General Public License
 * as published by the Free Software Foundation; version 2
 * of the License or (at your option) a later version of the License.
 * 
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 * 
 * You should have received a copy of the GNU General Public License
 * along with this program; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston,
 * MA  02110-1301, USA.
 */

#include <Python.h>

#define hexbyte(x) (isdigit(x)?(x)-'0':(x)-'a'+0xa)
#define bytehex(x) (((x)<0xa)?('0'+(x)):('a'-0xa+(x)))

static PyObject *py_hex_to_sha(PyObject *self, PyObject *py_hexsha)
{
	char *hexsha;
	char sha[20];
	int i;

	if (!PyString_Check(py_hexsha)) {
		PyErr_SetString(PyExc_TypeError, "hex sha is not a string");
		return NULL;
	}

	if (PyString_Size(py_hexsha) != 40) {
		PyErr_SetString(PyExc_ValueError, "hex sha is not 40 bytes long");
		return NULL;
	}

	hexsha = PyString_AsString(py_hexsha);

	for (i = 0; i < 20; i++) {
		sha[i] = (hexbyte(hexsha[i*2]) << 4) + hexbyte(hexsha[i*2+1]);
	}

	return PyString_FromStringAndSize(sha, 20);
}

static PyObject *py_sha_to_hex(PyObject *self, PyObject *py_sha)
{
	char hexsha[41];
	unsigned char *sha;
	int i;

	if (!PyString_Check(py_sha)) {
		PyErr_SetString(PyExc_TypeError, "sha is not a string");
		return NULL;
	}

	if (PyString_Size(py_sha) != 20) {
		PyErr_SetString(PyExc_ValueError, "sha is not 20 bytes long");
		return NULL;
	}

	sha = (unsigned char *)PyString_AsString(py_sha);
	for (i = 0; i < 20; i++) {
		hexsha[i*2] = bytehex((sha[i] & 0xF0) >> 4);
		hexsha[i*2+1] = bytehex(sha[i] & 0x0F);
	}
	
	return PyString_FromStringAndSize(hexsha, 40);
}

static PyMethodDef py_objects_methods[] = {
	{ "hex_to_sha", (PyCFunction)py_hex_to_sha, METH_O, NULL },
	{ "sha_to_hex", (PyCFunction)py_sha_to_hex, METH_O, NULL },
};

void init_objects(void)
{
	PyObject *m;

	m = Py_InitModule3("_objects", py_objects_methods, NULL);
	if (m == NULL)
		return;
}
