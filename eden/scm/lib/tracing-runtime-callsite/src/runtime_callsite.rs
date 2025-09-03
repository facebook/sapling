/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::sync::Once;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use tracing::Event;
use tracing::Metadata;
use tracing::Span;
use tracing::callsite::Callsite;
use tracing::callsite::Identifier;
use tracing::field::Field;
use tracing::field::FieldSet;
use tracing::field::Value;
use tracing::subscriber::Interest;

use crate::CallsiteInfo;
use crate::EventKindType;
use crate::Intern;
use crate::KindType;
use crate::SpanKindType;
use crate::StaticBox;
use crate::array::Array;
use crate::call_array;

/*

The "Callsite" API is complex. To help explain it, let's look at an example
of expanded code of `tracing::info_span!`:

    let span = {
        use ::tracing::__macro_support::Callsite as _;
        static CALLSITE: ::tracing::__macro_support::MacroCallsite = {
            use ::tracing::__macro_support::MacroCallsite;
            static META: ::tracing::Metadata<'static> = {
                ::tracing_core::metadata::Metadata::new(
                    "foo",
                    "tracing_test",
                    ::tracing::Level::INFO,
                    Some("src\\main.rs"),
                    Some(3u32),
                    Some("tracing_test"),
                    ::tracing_core::field::FieldSet::new(
                        &[],
                        ::tracing_core::callsite::Identifier(&CALLSITE),
                    ),
                    ::tracing::metadata::Kind::SPAN,
                )
            };
            MacroCallsite::new(&META)
        };
        let mut interest = ::tracing::subscriber::Interest::never();
        if ::tracing::Level::INFO <= ::tracing::level_filters::STATIC_MAX_LEVEL
            && ::tracing::Level::INFO <= ::tracing::level_filters::LevelFilter::current()
            && {
                interest = CALLSITE.interest();
                !interest.is_never()
            }
            && CALLSITE.is_enabled(interest)
        {
            let meta = CALLSITE.metadata();
            ::tracing::Span::new(meta, &{ meta.fields().value_set(&[]) })
        } else {
            let span = CALLSITE.disabled_span();
            {};
            span
        }
    };

Definition of `MacroCallsite` (tracing 0.1.22):

    #[derive(Debug)]
    pub struct MacroCallsite {
        interest: AtomicUsize,
        meta: &'static Metadata<'static>,
        registration: Once,
    }


In short:
- FieldSet: static field names, and a Callsite trait object (!).
- Metadata: static names, line number, etc, and FieldSet.
- Callsite: a cached "is_disabled" state called "interest", and Metadata.

Yes, Callsite refers to itself via Metadata -> FieldSet.

The Span creation usually takes advantage of the cached "interest"
for performance, as the expanded macro shows.

*/

/// The main type. Implements the `tracing::Callsite` trait, and can be
/// created at runtime.
///
/// It is private to the crate to ensure it can only be created to keep the
/// 'static references alive.
pub struct RuntimeCallsite<K> {
    /// Contains field names.
    meta: StaticBox<Metadata<'static>>,

    /// Keeps the referred data alive.
    #[allow(dead_code)]
    owned: StaticBox<CallsiteOwned>,

    /// Interest, aka. "cached enabled". Part of the MacroCallsite API.
    ///
    /// `tracing_core::callsite::rebuild_interest_cache()` can invalid it.
    interest: AtomicUsize,

    /// State about whether registration was done. Part of the MacroCallsite API.
    registration: Once,

    phantom: PhantomData<K>,
}

/// Values referred by Metadata and FieldSet. Subset of CallsiteInfo.
struct CallsiteOwned {
    ref_field_names: Vec<&'static str>,
    fields: Vec<Field>,
}

impl<K: KindType> RuntimeCallsite<K> {
    /// Construct the callsite. For public API, use `crate::create_callsite` instead,
    /// which ensures created values aren't released within safe code.
    pub(crate) fn new(info: CallsiteInfo) -> StaticBox<Self> {
        let CallsiteInfo {
            name,
            target,
            level,
            file,
            line,
            module_path,
            field_names,
        } = info;
        let mut owned = StaticBox::new(CallsiteOwned {
            ref_field_names: field_names.iter().map(|s| s.intern()).collect(),
            fields: Default::default(),
        });

        // To construct Callsite, &Callsite is needed (for FieldSet, then Metadata).
        // MaybeUninit allows us to get &Callsite without constructing it.
        let mut callsite: StaticBox<MaybeUninit<Self>> = StaticBox::new(MaybeUninit::uninit());
        let identifier = {
            let site: &'static MaybeUninit<Self> = callsite.static_ref();
            // safety: MaybeUninit<T> and T have the same ABI. The uninit memory is not
            // read until initialized. NOTE: This relies on FieldSet::new to not read the
            // Callsite, which is true as of tracing 0.1.22.
            let site: &'static Self = unsafe { &*site.as_ptr() };
            site.identifier()
        };

        let fields = FieldSet::new(owned.static_ref().ref_field_names.as_slice(), identifier);
        let meta = {
            StaticBox::new(Metadata::new(
                name.intern(),
                target.intern(),
                level,
                file.intern(),
                line,
                module_path.intern(),
                fields,
                K::kind(),
            ))
        };

        // FieldSet::iter() produces Field on the fly, not &Field! We have to store
        // those `Field`s so they can have static references.
        owned.as_mut().fields = meta.fields().iter().collect::<Vec<_>>();

        // Construct Self.
        let callsite: StaticBox<Self> = unsafe {
            callsite.as_mut().as_mut_ptr().write(Self {
                meta,
                owned,
                interest: AtomicUsize::new(3 /* undecided */),
                registration: Once::new(),
                phantom: PhantomData,
            });
            // safety: MaybeUninit<T> and T have the same ABI.
            let callsite: StaticBox<MaybeUninit<Self>> = callsite;
            std::mem::transmute(callsite)
        };

        callsite
    }

    pub(crate) fn identifier(&'static self) -> Identifier {
        Identifier(self as &'static dyn Callsite)
    }

    /// Register to the global linked list. Can be slow.
    #[inline(never)]
    #[cold]
    fn register(&'static self) -> Interest {
        self.registration
            .call_once(|| tracing::callsite::register(self));
        // set_interest should be called.
        match self.interest.load(Ordering::Relaxed) {
            0 => Interest::never(),
            2 => Interest::always(),
            1 => Interest::sometimes(),
            _ => panic!("set_interest was not called by tracing::callsite::register!"),
        }
    }

    /// Test if the callsite is enabled. This has some caching, fast paths,
    /// mimics the behavior of the static macro.
    pub fn is_enabled(&'static self) -> bool {
        match self.interest.load(Ordering::Relaxed) {
            0 => Some(false),
            1 => None, /* sometimes */
            2 => Some(true),
            _ => {
                // TODO: Uncomment when tracing >= 0.1.18.
                /*
                let level = self.meta.level();
                if level > LevelFilter::current() {
                    Some(false)
                } else */
                {
                    // register() can be slow, but is one-time.
                    let interest = self.register();
                    if interest.is_never() {
                        Some(false)
                    } else if interest.is_always() {
                        Some(true)
                    } else {
                        None
                    }
                }
            }
        }
        .unwrap_or_else(|| {
            // Interest::sometimes(), slow path, is not one-time (!).
            tracing::dispatcher::get_default(|default| default.enabled(&self.meta))
        })
    }

    /// Associate field names with values.
    ///
    /// The length of the values should match the `field_names` passed to
    /// `CallsiteInfo`. `None` means the field does not exist.
    fn field_with_values<'v>(
        &self,
        values: &'v [Option<Box<dyn Value + 'v>>],
    ) -> Vec<(&'static Field, Option<&(dyn Value + 'v)>)> {
        // This type signature is required by FieldSet.value_set.
        let field_values: Vec<(&'static Field, Option<&(dyn Value + 'v)>)> = self
            .owned
            .static_ref()
            .fields
            .iter()
            .zip(values.iter().map(|o| o.as_ref().map(|v| v.as_ref())))
            .collect();
        field_values
    }
}

impl RuntimeCallsite<SpanKindType> {
    /// Create a [`Span`] with fields set to the given values (if enabled).
    /// This mimics the macro version to for fast paths.
    pub fn create_span<'v>(&'static self, values: &'v [Option<Box<dyn Value + 'v>>]) -> Span {
        if self.is_enabled() {
            let field_values = self.field_with_values(values);
            let field_values: Array<_> = field_values.into();
            Span::new(&self.meta, &{
                let fieldset = self.meta.fields();
                call_array!(fieldset.value_set(&field_values))
            })
        } else {
            Span::none()
        }
    }
}

impl RuntimeCallsite<EventKindType> {
    /// Create a [`Event`] with fields set to the given values.
    pub fn create_event<'v>(&'static self, values: &'v [Option<Box<dyn Value + 'v>>]) {
        if self.is_enabled() {
            let field_values = self.field_with_values(values);
            let field_values: Array<_> = field_values.into();
            Event::dispatch(&self.meta, &{
                let fieldset = self.meta.fields();
                call_array!(fieldset.value_set(&field_values))
            });
        }
    }
}

impl<K: KindType> Callsite for RuntimeCallsite<K> {
    fn set_interest(&self, interest: Interest) {
        let interest = match () {
            _ if interest.is_never() => 0,
            _ if interest.is_always() => 2,
            _ => 1,
        };
        self.interest.store(interest, Ordering::SeqCst);
    }

    fn metadata(&self) -> &Metadata<'_> {
        &self.meta
    }
}
