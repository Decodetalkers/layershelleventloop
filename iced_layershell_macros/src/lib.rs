use darling::{
    ast::{Data, NestedMeta},
    util::{Flag, Ignored},
    FromDeriveInput, FromMeta,
};
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use syn::{
    parse_macro_input, Attribute, Data as DataOrigin, DataEnum, DeriveInput, Generics, Ident,
    LitStr, Variant, Visibility,
};

use quote::quote;

#[inline]
fn is_singleton_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("singleton")
}

#[inline]
fn is_mainwindow_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("main")
}

/// WindowInfoMarker, it is a derive to mark the WIndowInfo of MultiApplication.
/// There are two attributes: singleton and main
/// Singleton is used to mark the window can only exist once,
/// main is used to get the id of the main window
#[proc_macro_derive(WindowInfoMarker, attributes(singleton, main))]
pub fn window_info_marker(input: TokenStream) -> TokenStream {
    // Parse the input as a DeriveInput
    let input = parse_macro_input!(input as DeriveInput);

    // Get the name of the enum
    let name = input.ident.clone();

    // Ensure the macro is applied to an enum
    let variants = if let DataOrigin::Enum(DataEnum { variants, .. }) = input.data {
        variants
    } else {
        return syn::Error::new_spanned(input, "Singleton macro can only be applied to enums")
            .to_compile_error()
            .into();
    };

    // Generate the implementation of `IsSingleton`
    let is_singleton_arms = variants.iter().map(|variant| {
        let variant_name = &variant.ident;

        // Check if the variant has the `#[singleton]` attribute
        let is_singleton = variant.attrs.iter().any(is_singleton_attr);

        match &variant.fields {
            syn::Fields::Unit => quote! { Self::#variant_name => #is_singleton, },
            syn::Fields::Unnamed(_) => quote! { Self::#variant_name(..) => #is_singleton, },
            syn::Fields::Named(_) => quote! { Self::#variant_name { .. } => #is_singleton, },
        }
    });

    let try_from_mainwindow = variants
        .iter()
        .find(|variant| variant.attrs.iter().any(is_mainwindow_attr))
        .map(|variant| {
            let variant_name = &variant.ident;
            quote! {
                impl TryFrom<iced_layershell::actions::MainWindowInfo> for #name {
                    type Error = ();
                    fn try_from(_val: iced_layershell::actions::MainWindowInfo) -> Result<Self, ()> {
                        Ok(Self::#variant_name)
                    }
                }
            }
        })
        .unwrap_or(quote! {
            impl TryFrom<iced_layershell::actions::MainWindowInfo> for #name {
                type Error = ();
                fn try_from(_val: iced_layershell::actions::MainWindowInfo) -> Result<Self, ()> {
                    Err(())
                }
            }
        });

    // Generate the final implementation
    let expanded = quote! {
        impl iced_layershell::actions::IsSingleton for #name {
            fn is_singleton(&self) -> bool {
                match self {
                    #(#is_singleton_arms)*
                }
            }
        }
        #try_from_mainwindow
    };

    TokenStream::from(expanded)
}

/// to_layer_message is to convert a normal enum to the enum usable in iced_layershell
/// It impl the try_into trait for the enum and make it can be convert to the actions in
/// layershell.
///
/// It will automatic add the fields which match the actions in iced_layershell
#[manyhow::manyhow]
#[proc_macro_attribute]
pub fn to_layer_message(attr: TokenStream2, input: TokenStream2) -> manyhow::Result<TokenStream2> {
    let meta = NestedMeta::parse_meta_list(attr)?;

    let ToLayerMessageAttr { multi, info_name } = ToLayerMessageAttr::from_list(&meta)?;

    let is_multi = multi.is_present();

    let derive_input = syn::parse2::<DeriveInput>(input)?;
    let attrs = &derive_input.attrs;
    let MessageEnum {
        vis,
        ident,
        generics,
        data,
    } = MessageEnum::from_derive_input(&derive_input)?;

    let (impl_gen, ty_gen, where_gen) = generics.split_for_impl();
    let variants = data.take_enum().unwrap();

    let (additional_variants, try_into_impl) = match is_multi {
        true => {
            let info_name = info_name.expect("Should set the info_name").value();
            let info = Ident::new(&info_name, Span::call_site());

            let additional_variants = quote! {
                AnchorChange{id: iced::window::Id, anchor: iced_layershell::reexport::Anchor},
                AnchorSizeChange{id: iced::window::Id, anchor:iced_layershell::reexport::Anchor, size: (u32, u32)},
                LayerChange{id: iced::window::Id, layer:iced_layershell::reexport::Layer},
                MarginChange{id: iced::window::Id, margin: (i32, i32, i32, i32)},
                SizeChange{id: iced::window::Id, size: (u32, u32)},
                VirtualKeyboardPressed {
                    time: u32,
                    key: u32,
                },
                NewLayerShell { settings: iced_layershell::reexport::NewLayerShellSettings, info: #info },
                NewPopUp { settings: iced_layershell::actions::IcedNewPopupSettings, info: #info },
                NewMenu { settings: iced_layershell::actions::IcedNewMenuSettings, info: #info },
                RemoveWindow(iced::window::Id),
                ForgetLastOutput,
            };

            let try_into_impl = quote! {
                impl #impl_gen TryInto<iced_layershell::actions::LayershellCustomActionsWithIdAndInfo<#info>> for #ident #ty_gen #where_gen {
                    type Error = Self;

                    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActionsWithIdAndInfo<#info>, Self::Error> {
                        type InnerLayerActionId = iced_layershell::actions::LayershellCustomActionsWithIdAndInfo<#info>;
                        type InnerLayerAction = iced_layershell::actions::LayershellCustomActionsWithInfo<#info>;

                        match self {
                            Self::AnchorChange { id, anchor } => Ok(InnerLayerActionId::new(Some(id), InnerLayerAction::AnchorChange(anchor))),
                            Self::AnchorSizeChange { id, anchor, size } => Ok(InnerLayerActionId::new(Some(id), InnerLayerAction::AnchorSizeChange(anchor, size))),
                            Self::LayerChange { id, layer } => Ok(InnerLayerActionId::new(Some(id), InnerLayerAction::LayerChange(layer))),
                            Self::MarginChange { id, margin } => Ok(InnerLayerActionId::new(Some(id), InnerLayerAction::MarginChange(margin))),
                            Self::SizeChange { id, size } => Ok(InnerLayerActionId::new(Some(id), InnerLayerAction::SizeChange(size))),
                            Self::VirtualKeyboardPressed { time, key } => Ok(InnerLayerActionId::new(
                                None,
                                InnerLayerAction::VirtualKeyboardPressed { time, key })
                            ),
                            Self::NewLayerShell {settings, info } => Ok(InnerLayerActionId::new(None, InnerLayerAction::NewLayerShell { settings, info })),
                            Self::NewPopUp { settings, info } => Ok(InnerLayerActionId::new(None, InnerLayerAction::NewPopUp { settings, info })),
                            Self::NewMenu { settings, info } =>  Ok(InnerLayerActionId::new(None, InnerLayerAction::NewMenu {settings, info })),
                            Self::RemoveWindow(id) => Ok(InnerLayerActionId::new(None, InnerLayerAction::RemoveWindow(id))),
                            Self::ForgetLastOutput => Ok(InnerLayerActionId::new(None, InnerLayerAction::ForgetLastOutput)),
                            _ => Err(self)
                        }
                    }
                }
            };

            (additional_variants, try_into_impl)
        }
        false => {
            let additional_variants = quote! {
                AnchorChange(iced_layershell::reexport::Anchor),
                AnchorSizeChange(iced_layershell::reexport::Anchor, (u32, u32)),
                LayerChange(iced_layershell::reexport::Layer),
                MarginChange((i32, i32, i32, i32)),
                SizeChange((u32, u32)),
                VirtualKeyboardPressed {
                    time: u32,
                    key: u32,
                },
            };
            let try_into_impl = quote! {
                impl #impl_gen TryInto<iced_layershell::actions::LayershellCustomActions> for #ident #ty_gen #where_gen {
                    type Error = Self;

                    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self::Error> {
                        use iced_layershell::actions::LayershellCustomActions;

                        match self {
                            Self::AnchorChange(anchor) => Ok(LayershellCustomActions::AnchorChange(anchor)),
                            Self::AnchorSizeChange(anchor, size) => Ok(LayershellCustomActions::AnchorSizeChange(anchor, size)),
                            Self::LayerChange(layer) => Ok(LayershellCustomActions::LayerChange(layer)),

                            Self::MarginChange(margin) => Ok(LayershellCustomActions::MarginChange(margin)),
                            Self::SizeChange(size) => Ok(LayershellCustomActions::SizeChange(size)),
                            Self::VirtualKeyboardPressed { time, key } => Ok(LayershellCustomActions::VirtualKeyboardPressed {
                                time,
                                key
                            }),
                            _ => Err(self)
                        }
                    }
                }
            };

            (additional_variants, try_into_impl)
        }
    };

    Ok(quote! {
        #(#attrs)*
        #vis enum #ident #ty_gen #where_gen {
            #(#variants,)*
            #additional_variants
        }

        #try_into_impl
    })
}

#[derive(FromMeta)]
struct ToLayerMessageAttr {
    multi: Flag,
    info_name: Option<LitStr>,
}

#[derive(FromDeriveInput)]
#[darling(supports(enum_any))]
struct MessageEnum {
    vis: Visibility,
    ident: Ident,
    generics: Generics,
    data: Data<Variant, Ignored>,
}
