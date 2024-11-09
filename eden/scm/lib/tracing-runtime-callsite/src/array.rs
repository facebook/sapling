/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Represent a dynamic length array in an enum so we can get back a fixed sized array.
///
/// The complexity is forced by the `ValidLen` trait of `tracing::FieldSet::value_set`.
#[derive(Clone)]
pub(crate) enum Array<T> {
    /* [[[cog
        import cog
        n = 12
        for i in range(0, n):
            cog.outl(f"Len{i}([T; {i}]),")
    ]]] */
    Len0([T; 0]),
    Len1([T; 1]),
    Len2([T; 2]),
    Len3([T; 3]),
    Len4([T; 4]),
    Len5([T; 5]),
    Len6([T; 6]),
    Len7([T; 7]),
    Len8([T; 8]),
    Len9([T; 9]),
    Len10([T; 10]),
    Len11([T; 11]),
    /* [[[end]]] */
}

impl<T: Clone> Array<T> {
    #[allow(dead_code)]
    fn as_slice(&self) -> &[T] {
        match self {
            /* [[[cog
                import cog
                n = 12
                for i in range(0, n):
                    cog.outl(f"Array::Len{i}(a) => &a[..],")
            ]]] */
            Array::Len0(a) => &a[..],
            Array::Len1(a) => &a[..],
            Array::Len2(a) => &a[..],
            Array::Len3(a) => &a[..],
            Array::Len4(a) => &a[..],
            Array::Len5(a) => &a[..],
            Array::Len6(a) => &a[..],
            Array::Len7(a) => &a[..],
            Array::Len8(a) => &a[..],
            Array::Len9(a) => &a[..],
            Array::Len10(a) => &a[..],
            Array::Len11(a) => &a[..],
            /* [[[end]]] */
        }
    }
}

impl<T> Default for Array<T> {
    fn default() -> Self {
        Self::Len0([])
    }
}

impl<T> From<Vec<T>> for Array<T> {
    fn from(v: Vec<T>) -> Self {
        let len = v.len();
        let mut i = v.into_iter();
        let mut n = move || i.next().unwrap();
        match len {
            /* [[[cog
                import cog
                n = 12
                for i in range(0, n):
                    body = ", ".join(["n()"] * i)
                    pat = i
                    if i == n - 1:
                        pat = "_"
                    cog.outl(f"{pat} => Array::Len{i}([{body}]),")
            ]]] */
            0 => Array::Len0([]),
            1 => Array::Len1([n()]),
            2 => Array::Len2([n(), n()]),
            3 => Array::Len3([n(), n(), n()]),
            4 => Array::Len4([n(), n(), n(), n()]),
            5 => Array::Len5([n(), n(), n(), n(), n()]),
            6 => Array::Len6([n(), n(), n(), n(), n(), n()]),
            7 => Array::Len7([n(), n(), n(), n(), n(), n(), n()]),
            8 => Array::Len8([n(), n(), n(), n(), n(), n(), n(), n()]),
            9 => Array::Len9([n(), n(), n(), n(), n(), n(), n(), n(), n()]),
            10 => Array::Len10([n(), n(), n(), n(), n(), n(), n(), n(), n(), n()]),
            _ => Array::Len11([n(), n(), n(), n(), n(), n(), n(), n(), n(), n(), n()]),
            /* [[[end]]] */
        }
    }
}

#[macro_export]
macro_rules! call_array {
    ($e0:ident $(.$e:ident)* ($a:expr)) => {
        match $a {
            /* [[[cog
                import cog
                n = 12
                for i in range(0, n):
                    cog.outl(f"$crate::array::Array::Len{i}(a) => $e0 $(.$e)* (a),")
            ]]] */
            $crate::array::Array::Len0(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len1(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len2(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len3(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len4(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len5(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len6(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len7(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len8(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len9(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len10(a) => $e0 $(.$e)* (a),
            $crate::array::Array::Len11(a) => $e0 $(.$e)* (a),
            /* [[[end]]] */
        }
    }
}
