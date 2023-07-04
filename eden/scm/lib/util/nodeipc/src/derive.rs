/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;

pub use anyhow;
pub use paste::paste;
use serde::Deserialize;
use serde::Serialize;
pub use serde_json;
pub use serde_json::Value;

use crate::NodeIpc;

/// Serialiable message over IPC.
#[derive(Serialize, Deserialize)]
pub enum IpcMessage {
    /// The sender requests the receiver to evaluate a function call.
    Call(CallArgs),
    /// The sender completes a `Call` and is returning its result.
    Return(ReturnArgs),
}

/// Calling a function. Name and arguments.
#[derive(Serialize, Deserialize)]
pub struct CallArgs(pub Cow<'static, str>, pub Value);

/// Return value of a function call.
#[derive(Serialize, Deserialize)]
pub struct ReturnArgs(pub Value);

/// Indicates a struct contains `NodeIpc`.
pub trait HasIpc {
    /// Get the `NodeIpc`.
    fn ipc(&self) -> &NodeIpc;
}

/// "Dynamic" dispatch.
pub trait Call {
    /// "Dynamic" dispatch. Input and output are normalized to `Value`.
    fn call(&self, value: CallArgs) -> anyhow::Result<Value>;
}

/// Main serve loop. Auto implemented for `HasIpc` + `Call`.
pub trait Serve {
    /// Serve a client until it disconnects.
    fn serve(&self) -> anyhow::Result<()>;
}

impl<T: HasIpc + Call> Serve for T {
    fn serve(&self) -> anyhow::Result<()> {
        let ipc = self.ipc();
        loop {
            let message = match ipc.recv::<IpcMessage>() {
                Ok(Some(v)) => v,
                // Err(_) can also be caused by "end of stream".
                Ok(None) | Err(_) => break,
            };
            match message {
                IpcMessage::Call(call) => {
                    let value = self.call(call)?;
                    ipc.send(IpcMessage::Return(ReturnArgs(value)))?;
                }
                IpcMessage::Return(ReturnArgs(ret)) => {
                    anyhow::bail!("Unmatched return {:?}", ret);
                }
            }
        }
        Ok(())
    }
}

impl HasIpc for NodeIpc {
    fn ipc(&self) -> &NodeIpc {
        self
    }
}

impl Call for NodeIpc {
    fn call(&self, _value: CallArgs) -> anyhow::Result<Value> {
        anyhow::bail!("Call::call is not supported on NodeIpc")
    }
}

/// Derive IPC methods.
///
/// For example, the following code:
///
/// ```rust,ignore
/// define_ipc! {
///     impl PlusService {
///         fn plus(&self, a: i32, b: i32) -> i32 {
///             a + b
///         }
///     }
/// }
/// ```
///
/// will generate both the server and client code:
///
/// ```rust,ignore
/// // Server. Requires `HasIpc`. Use `Serve::serve` to start the service.
/// impl Call for PlusService {
///     ...
/// }
///
/// // Client. Requires `HasIpc` and `Call` (in case the server calls methods back).
/// trait PlusServiceIpc {
///     fn plus(&self, a: i32, b: i32) -> anyhow::Result<i32> {
///         ...
///     }
/// }
/// ```
///
/// The IPC is designed to be blocking, sequential, and bi-directional.
/// The client also provides methods (`Call`) the server can call.
/// See the unit test for an example.
#[macro_export]
macro_rules! define_ipc {
    {
        impl $impl_name:ident $(< $impl_lifetime:lifetime >)? {
            $(
                $(#[$fn_meta:meta])*
                $fn_vis:vis fn $fn_name:ident (&$self:ident $(, $fn_arg_name:ident : $fn_arg_type:ty)* ) -> $fn_ret:ty
                {
                    $($fn_body:tt)*
                }
            )*
        }
    } => {
        // Original code.
        impl $impl_name $(< $impl_lifetime >)? {
            $(
                $(#[$fn_meta])*
                $fn_vis fn $fn_name (&$self $(, $fn_arg_name : $fn_arg_type)* ) -> $fn_ret
                {
                    $($fn_body)*
                }
            )*
        }

        // Derived logic for server (Serve::serve).
        impl $crate::derive::Call for $impl_name $(< $impl_lifetime >)? {
            fn call(&self, value: $crate::derive::CallArgs) -> $crate::derive::anyhow::Result<$crate::derive::Value> {
                #[allow(unused_variables)]
                let $crate::derive::CallArgs(fn_name, fn_args) = value;
                $(
                    if fn_name == stringify!($fn_name) {
                        let ( $($fn_arg_name,)* ) = $crate::derive::serde_json::from_value(fn_args)?;
                        let result = self. $fn_name (  $($fn_arg_name,)* );
                        let value = $crate::derive::serde_json::to_value(result)?;
                        return Ok(value);
                    }
                )*
                $crate::derive::anyhow::bail!("{} received unknown method {}", stringify!($impl_name), fn_name);
            }
        }

        // Derived logic for client (NodeIpc).
        $crate::derive::paste! {
            /// Generated trait to call methods via ipc.
            pub trait [< $impl_name Ipc >] {
                $(
                    fn $fn_name (&self $(, $fn_arg_name : $fn_arg_type)* ) -> $crate::derive::anyhow::Result<$fn_ret>;
                )*
            }

            impl<T> [< $impl_name Ipc >] for T
            where
                T: $crate::derive::HasIpc,
                T: $crate::derive::Call,
            {
                $(
                    fn $fn_name (&self $(, $fn_arg_name : $fn_arg_type)* ) -> $crate::derive::anyhow::Result<$fn_ret> {
                        use $crate::derive::IpcMessage;
                        use $crate::derive::ReturnArgs;
                        use $crate::derive::serde_json;
                        let args = serde_json::to_value(($($fn_arg_name,)*))?;
                        let msg = IpcMessage::Call($crate::derive::CallArgs(::std::borrow::Cow::Borrowed(stringify!($fn_name)), args));
                        self.ipc().send(msg)?;
                        loop {
                            let received = self.ipc().recv::<$crate::derive::IpcMessage>()?;
                            match received {
                                Some(IpcMessage::Call(call)) => {
                                    let value = self.call(call)?;
                                    self.ipc().send(IpcMessage::Return(ReturnArgs(value)))?;
                                }
                                Some(IpcMessage::Return(ReturnArgs(value))) => {
                                    let ret: $fn_ret = serde_json::from_value(value)?;
                                    return Ok(ret);
                                }
                                None => $crate::derive::anyhow::bail!("Did not get a response of {}::{}", stringify!($impl_name), stringify!($fn_name))
                            }
                        }
                    }
                )*
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;
    use crate::ipc;
    use crate::NodeIpc;

    #[test]
    fn test_mutural_recursive() {
        struct EvenService<'a>(NodeIpc, &'a (dyn (Fn(u32) -> Option<bool>) + Send + Sync));
        struct OddService(NodeIpc);

        define_ipc! {
            impl EvenService<'_> {
                fn is_even(&self, n: u32) -> bool {
                    if let Some(result) = (self.1)(n) {
                        return result;
                    }

                    // self.is_odd() is provided by the generated OddServiceIpc trait.
                    self.is_odd(n - 1).unwrap()
                }
            }
        }

        #[ipc(test)]
        impl OddService {
            pub fn is_odd(&self, n: u32) -> bool {
                // self.is_even() is provided by the generated EvenServiceIpc trait.
                self.is_even(n - 1).unwrap()
            }
        }

        impl<'a> HasIpc for EvenService<'a> {
            fn ipc(&self) -> &NodeIpc {
                &self.0
            }
        }

        impl HasIpc for OddService {
            fn ipc(&self) -> &NodeIpc {
                &self.0
            }
        }

        let (server_socket, client_socket) = filedescriptor::socketpair().unwrap();
        let server_ipc = NodeIpc::from_socket(server_socket).unwrap();
        let client_ipc = NodeIpc::from_socket(client_socket).unwrap();

        let is_even = |v: u32| match v {
            1 => Some(false),
            0 => Some(true),
            _ => None,
        };
        let even_service = EvenService(server_ipc, &is_even);
        let odd_service = OddService(client_ipc);

        thread::scope(|s| {
            s.spawn(|| {
                even_service.serve().unwrap();
            });

            // This will call the EvenService (server).
            // The EvenService (server) will ask OddService (client) questions too.
            let v = odd_service.is_odd(10);
            assert!(!v);
            drop(odd_service);
        })
    }
}
