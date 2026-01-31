#[macro_export]
macro_rules! const_tagged_enum {
    (
        #[const_tagged_enum(
            impl_module = $impl_mod: ident,
            derive(Debug)
        )]
        enum $name: ident $(<$($generic_params: tt),*> $(where [$($generic_bounds: tt)*])?)? {
            $($variant_name: ident ($variant_ty: ty) = $variant_discriminant: expr),* $(,)?
        }
    ) => {
        mod $impl_mod {
            use super::*;

            #[allow(non_snake_case)]
            union ImplUnion $($(<$generic_params),*> $(where $($generic_bounds)*)?)? {
                $($variant_name: std::mem::ManuallyDrop<$variant_ty>,)*
            }

            #[repr(C)]
            pub(super) struct $name <$($($generic_params,)*)? const VARIANT: u32> $($(where $($generic_bounds)*)?)? (ImplUnion $($(<$generic_params),*>)?);

            $crate::const_tagged_enum!(@impl_variants
                [$($($generic_params),*)?] [$($([$($generic_bounds)*])?)?]
                $name $($variant_name($variant_ty) = $variant_discriminant)*
            );

            #[allow(dead_code)]
            #[allow(non_snake_case)]
            impl<$($($generic_params,)*)? const VARIANT: u32> $name<$($($generic_params,)*)? VARIANT> where $($(where $($generic_bounds)*)?)? {
                $crate::const_tagged_enum!(@impl_dispatch_inner_broadcast_generics
                    [$($($generic_params),*)?]
                    $name $($variant_name($variant_ty) = $variant_discriminant),*
                );
            }

            #[allow(non_snake_case)]
            #[automatically_derived]
            impl<$($($generic_params,)*)? const VARIANT: u32> std::fmt::Debug for $name<$($($generic_params,)*)? VARIANT> where $($(where $($generic_bounds)*)?)? {
                #[inline]
                fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    Self::dispatch_with(self, f, $(|f, $variant_name| std::fmt::Debug::fmt(&*$variant_name, f)),*)
                }
            }

            #[automatically_derived]
            impl<$($($generic_params,)*)? const VARIANT: u32> Drop for $name<$($($generic_params,)*)? VARIANT> where $($(where $($generic_bounds)*)?)? {
                #[inline]
                fn drop(&mut self) {
                    const { assert!($(VARIANT == $variant_discriminant)||*, "Const discriminant is not one of the allowed values"); }

                    $(if const { VARIANT == $variant_discriminant } {
                        return unsafe { std::mem::ManuallyDrop::drop(&mut self.0.$variant_name); };
                    })*

                    unreachable!();
                }
            }
        }

        use $impl_mod::$name;
    };
    // workaround for macro repetition limitations
    // have to temporarily "squash" generic params to a tt so the compiler doesn't complain
    // about repetition counts
    (@impl_variants
        $generic_params: tt $generic_bounds: tt $name: ident
        $($variant_name: ident($variant_ty: ty) = $variant_discriminant: expr)*
    ) => {
        $($crate::const_tagged_enum!(@impl_variant $generic_params $generic_bounds $name $variant_name($variant_ty) = $variant_discriminant);)*
    };
    (@impl_variant
        [$($generic_params: tt)*] [$([$($generic_bounds: tt)*])?] $name: ident
        $variant_name: ident($variant_ty: ty) = $variant_discriminant: expr
    ) => {
        #[allow(dead_code)]
        impl<$($generic_params),*> $name<$($generic_params,)* $variant_discriminant> where $($(where $($generic_bounds)*)?)? {
            #[inline]
            pub fn new(value: $variant_ty) -> Self {
                Self(ImplUnion {
                    $variant_name: std::mem::ManuallyDrop::new(value)
                })
            }

            #[inline]
            pub fn into_inner(mut this: Self) -> $variant_ty {
                let inner = unsafe { std::mem::ManuallyDrop::take(&mut this.0.$variant_name) };
                std::mem::forget(this);
                inner
            }
        }

        #[automatically_derived]
        impl<$($generic_params),*> std::ops::Deref for $name<$($generic_params,)* $variant_discriminant> where $($(where $($generic_bounds)*)?)? {
            type Target = $variant_ty;

            #[inline]
            fn deref(&self) -> &Self::Target {
                unsafe { &self.0.$variant_name }
            }
        }

        #[automatically_derived]
        impl<$($generic_params),*> std::ops::DerefMut for $name<$($generic_params,)* $variant_discriminant> where $($(where $($generic_bounds)*)?)? {
            #[inline]
            fn deref_mut(&mut self) -> &mut Self::Target {
                unsafe { &mut self.0.$variant_name }
            }
        }
    };
    (@impl_dispatch_inner_broadcast_generics
        $generic_params: tt
        $name: ident $($variant_name: ident($variant_ty: ty) = $variant_discriminant: expr),*
    ) => {
        $crate::const_tagged_enum!(@impl_dispatch_inner
            $name $($generic_params $variant_name($variant_ty) = $variant_discriminant),*
        );
    };
    (@impl_dispatch_inner
        $name: ident $([$($generic_params: tt)*] $variant_name: ident($variant_ty: ty) = $variant_discriminant: expr),*
    ) => {
        #[inline]
        pub fn dispatch_with<A, R>(&self, arg: A, $($variant_name: impl FnOnce(A, &$name<$($generic_params,)* $variant_discriminant> ) -> R),*) -> R {
            const { assert!($(VARIANT == $variant_discriminant)||*, "Const discriminant is not one of the allowed values"); }

            $(if const { VARIANT == $variant_discriminant } {
                // This transmute literally transmutes the value to the exact same type, just "conster".
                let concrete_ref = unsafe { std::mem::transmute::<&Self, &$name<$($generic_params,)* $variant_discriminant>>(&self) };
                return $variant_name(arg, concrete_ref);
            })*

            unreachable!();
        }

        #[inline]
        pub fn dispatch<R>(&self, $($variant_name: impl FnOnce(&$name<$($generic_params,)* $variant_discriminant> ) -> R),*) -> R {
            Self::dispatch_with(self, (), $(|_, v| $variant_name(v)),*)
        }
    };
}

#[cfg(test)]
mod test {
    const VALUE_TYPE_STRING: u32 = 0;
    const VALUE_TYPE_STRING_REF: u32 = 1;
    const VALUE_TYPE_NUMBER: u32 = 2;

    const_tagged_enum! {
        #[const_tagged_enum(
            impl_module = value_impl,
            derive(Debug)
        )]
        enum Value<'a> {
            String(String) = VALUE_TYPE_STRING,
            StringRef(&'a str) = VALUE_TYPE_STRING_REF,
            Number(u32) = VALUE_TYPE_NUMBER,
        }
    }

    const _: () = const {
        assert!(
            std::mem::size_of::<Value<VALUE_TYPE_NUMBER>>()
                == std::mem::size_of::<Value<VALUE_TYPE_STRING>>()
        );
        assert!(
            std::mem::align_of::<Value<VALUE_TYPE_NUMBER>>()
                == std::mem::align_of::<Value<VALUE_TYPE_STRING_REF>>()
        );
    };

    #[test]
    fn value_manipulation() {
        let string_ref = Value::<VALUE_TYPE_STRING_REF>::new("hello");
        assert_eq!(*string_ref, "hello");

        let mut string = Value::<VALUE_TYPE_STRING>::new("hello".into());
        string.push_str(" world");
        assert_eq!(*string, "hello world");
        let inner = Value::<VALUE_TYPE_STRING>::into_inner(string);
        assert_eq!(inner, "hello world");
    }
}
