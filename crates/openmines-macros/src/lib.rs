use proc_macro::TokenStream;
use quote::quote;
use rstml::node::{Node, NodeAttribute, NodeElement};
use rstml::parse2;
use syn::spanned::Spanned;

#[proc_macro]
pub fn gui(input: TokenStream) -> TokenStream {
    let tokens = proc_macro2::TokenStream::from(input);
    match parse2(tokens) {
        Ok(nodes) => {
            if nodes.is_empty() {
                return syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "Expected a single root element <window>",
                )
                .to_compile_error()
                .into();
            }
            let root = &nodes[0];
            match expand_node(root) {
                Ok(expanded) => expanded.into(),
                Err(err) => err.to_compile_error().into(),
            }
        }
        Err(err) => err.to_compile_error().into(),
    }
}

fn get_attr_expr(node: &NodeElement, name: &str) -> Option<syn::Expr> {
    node.attributes().iter().find_map(|attr| match attr {
        NodeAttribute::Attribute(keyed) if keyed.key.to_string() == name => {
            keyed.value().map(|val| match val {
                syn::Expr::Block(expr_block) => match expr_block.block.stmts.as_slice() {
                    [syn::Stmt::Expr(expr, _)] => expr.clone(),
                    _ => val.clone(),
                },
                _ => val.clone(),
            })
        }
        _ => None,
    })
}

fn get_single_child_or_value(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    if el.children.is_empty() {
        return Ok(quote! { "" });
    }
    if el.children.len() == 1 {
        match &el.children[0] {
            Node::Text(t) => {
                let s = t.value_string();
                Ok(quote! { #s })
            }
            Node::Block(b) => Ok(quote! { #b }),
            _ => Err(syn::Error::new(
                el.name().span(),
                "Expected text or expression inside element",
            )),
        }
    } else {
        let mut format_str = String::new();
        let mut format_exprs = Vec::new();
        for child in &el.children {
            match child {
                Node::Text(t) => {
                    format_str.push_str("{}");
                    let s = t.value_string();
                    format_exprs.push(quote! { #s });
                }
                Node::Block(b) => {
                    format_str.push_str("{}");
                    format_exprs.push(quote! { #b });
                }
                _ => return Err(syn::Error::new(child.span(), "Expected text or expression")),
            }
        }
        Ok(quote! {
            format!(#format_str, #(#format_exprs),*)
        })
    }
}

fn expand_window(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    let title = get_attr_expr(el, "title").ok_or_else(|| {
        syn::Error::new(
            el.name().span(),
            "Attribute 'title' is required for <window>",
        )
    })?;

    let mut child_tokens = Vec::new();
    for child in &el.children {
        child_tokens.push(expand_node(child)?);
    }

    let css_setup = get_attr_expr(el, "style")
        .map_or_else(|| quote! {}, |style| quote! { _horb = _horb.css(#style); });

    Ok(quote! {
        {
            let mut _horb = crate::net::session::ui::horb::Horb::new(#title);
            #css_setup
            #(#child_tokens)*
            _horb
        }
    })
}

fn expand_tabs(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut tabs = Vec::new();
    let tab_nodes = el.children.iter().filter_map(|child| match child {
        Node::Element(t_el) if t_el.name().to_string() == "tab" => Some(t_el),
        _ => None,
    });

    for t_el in tab_nodes {
        let label = get_attr_expr(t_el, "label").ok_or_else(|| {
            syn::Error::new(
                t_el.name().span(),
                "Attribute 'label' is required for <tab>",
            )
        })?;
        let action =
            get_attr_expr(t_el, "action").unwrap_or_else(|| syn::Expr::Verbatim(quote! { "" }));
        let active =
            get_attr_expr(t_el, "active").unwrap_or_else(|| syn::Expr::Verbatim(quote! { false }));
        tabs.push(quote! {
            _horb = _horb.tab(if #active {
                crate::net::session::ui::horb::Tab::active(#label)
            } else {
                crate::net::session::ui::horb::Tab::new(#label, #action)
            });
        });
    }
    Ok(quote! {
        #(#tabs)*
    })
}

fn expand_buttons(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut buttons = Vec::new();
    let btn_nodes = el.children.iter().filter_map(|child| match child {
        Node::Element(b_el) if b_el.name().to_string() == "button" => Some(b_el),
        _ => None,
    });

    for b_el in btn_nodes {
        let label = get_attr_expr(b_el, "label").ok_or_else(|| {
            syn::Error::new(
                b_el.name().span(),
                "Attribute 'label' is required for <button>",
            )
        })?;
        let action = get_attr_expr(b_el, "action").ok_or_else(|| {
            syn::Error::new(
                b_el.name().span(),
                "Attribute 'action' is required for <button>",
            )
        })?;
        buttons.push(quote! {
            _horb = _horb.button(crate::net::session::ui::horb::Button::new(#label, #action));
        });
    }
    Ok(quote! {
        #(#buttons)*
    })
}

fn expand_list(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut list_rows = Vec::new();
    let row_nodes = el.children.iter().filter_map(|child| match child {
        Node::Element(r_el) if r_el.name().to_string() == "row" => Some(r_el),
        _ => None,
    });

    for r_el in row_nodes {
        let title = get_attr_expr(r_el, "title").ok_or_else(|| {
            syn::Error::new(
                r_el.name().span(),
                "Attribute 'title' is required for <row>",
            )
        })?;
        let subtitle =
            get_attr_expr(r_el, "subtitle").unwrap_or_else(|| syn::Expr::Verbatim(quote! { "" }));
        let action =
            get_attr_expr(r_el, "action").unwrap_or_else(|| syn::Expr::Verbatim(quote! { "" }));
        list_rows.push(quote! {
            _horb = _horb.list_row(crate::net::session::ui::ListRow::new(#title, #subtitle, #action));
        });
    }
    Ok(quote! {
        #(#list_rows)*
    })
}

fn expand_text_row(label: &syn::Expr) -> proc_macro2::TokenStream {
    quote! {
        _horb = _horb.rich_row(crate::net::session::ui::horb::RichRow::text(#label));
    }
}

fn expand_toggle_row(
    r_el: &NodeElement,
    label: &syn::Expr,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let key = get_attr_expr(r_el, "key").ok_or_else(|| {
        syn::Error::new(
            r_el.name().span(),
            "Attribute 'key' is required for <toggle-row>",
        )
    })?;
    let active =
        get_attr_expr(r_el, "active").unwrap_or_else(|| syn::Expr::Verbatim(quote! { false }));
    Ok(quote! {
        _horb = _horb.rich_row(crate::net::session::ui::horb::RichRow::toggle(#label, #key, #active));
    })
}

fn expand_uint_row(
    r_el: &NodeElement,
    label: &syn::Expr,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let key = get_attr_expr(r_el, "key").ok_or_else(|| {
        syn::Error::new(
            r_el.name().span(),
            "Attribute 'key' is required for <uint-row>",
        )
    })?;
    let value = get_attr_expr(r_el, "value").unwrap_or_else(|| syn::Expr::Verbatim(quote! { 0 }));
    Ok(quote! {
        _horb = _horb.rich_row(crate::net::session::ui::horb::RichRow::uint(#label, #key, #value));
    })
}

fn expand_button_row(
    r_el: &NodeElement,
    label: &syn::Expr,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let btn_label = get_attr_expr(r_el, "btn-label").ok_or_else(|| {
        syn::Error::new(
            r_el.name().span(),
            "Attribute 'btn-label' is required for <button-row>",
        )
    })?;
    let action = get_attr_expr(r_el, "action").ok_or_else(|| {
        syn::Error::new(
            r_el.name().span(),
            "Attribute 'action' is required for <button-row>",
        )
    })?;
    Ok(quote! {
        _horb = _horb.rich_row(crate::net::session::ui::horb::RichRow::button(#label, #btn_label, #action));
    })
}

fn expand_dropdown_row(
    r_el: &NodeElement,
    label: &syn::Expr,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let key = get_attr_expr(r_el, "key").ok_or_else(|| {
        syn::Error::new(
            r_el.name().span(),
            "Attribute 'key' is required for <dropdown-row>",
        )
    })?;
    let selected =
        get_attr_expr(r_el, "selected").unwrap_or_else(|| syn::Expr::Verbatim(quote! { 0 }));

    let mut option_exprs = Vec::new();
    let opt_nodes = r_el.children.iter().filter_map(|opt| match opt {
        Node::Element(opt_el) if opt_el.name().to_string() == "option" => Some(opt_el),
        _ => None,
    });

    for opt_el in opt_nodes {
        let val = get_attr_expr(opt_el, "value").ok_or_else(|| {
            syn::Error::new(
                opt_el.name().span(),
                "Attribute 'value' is required for <option>",
            )
        })?;
        let lbl = get_attr_expr(opt_el, "label").ok_or_else(|| {
            syn::Error::new(
                opt_el.name().span(),
                "Attribute 'label' is required for <option>",
            )
        })?;
        option_exprs.push(quote! {
            format!("{}:{}", #val, #lbl)
        });
    }

    let options_format = if option_exprs.is_empty() {
        quote! { String::new() }
    } else {
        quote! {
            vec![#(#option_exprs),*].join("#") + "#"
        }
    };

    Ok(quote! {
        _horb = _horb.rich_row(crate::net::session::ui::horb::RichRow::dropdown(#label, #options_format, #key, #selected));
    })
}

fn expand_form(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut rich_rows = Vec::new();
    let row_nodes = el.children.iter().filter_map(|child| match child {
        Node::Element(r_el) => Some(r_el),
        _ => None,
    });

    for r_el in row_nodes {
        let r_tag = r_el.name().to_string();
        let label = get_attr_expr(r_el, "label").ok_or_else(|| {
            syn::Error::new(
                r_el.name().span(),
                "Attribute 'label' is required for form elements",
            )
        })?;

        match r_tag.as_str() {
            "text-row" => rich_rows.push(expand_text_row(&label)),
            "toggle-row" => rich_rows.push(expand_toggle_row(r_el, &label)?),
            "uint-row" => rich_rows.push(expand_uint_row(r_el, &label)?),
            "button-row" => rich_rows.push(expand_button_row(r_el, &label)?),
            "dropdown-row" => rich_rows.push(expand_dropdown_row(r_el, &label)?),
            _ => {
                return Err(syn::Error::new(
                    r_el.name().span(),
                    format!("Unknown form element <{r_tag}>"),
                ));
            }
        }
    }
    Ok(quote! {
        #(#rich_rows)*
    })
}

fn expand_canvas(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    let mut geom_tokens = Vec::new();
    if let Some(style) = get_attr_expr(el, "style") {
        geom_tokens.push(quote! {
            _horb = _horb.css(#style);
        });
    }
    let geom_nodes = el.children.iter().filter_map(|geom| match geom {
        Node::Element(g_el) => Some(g_el),
        _ => None,
    });

    for g_el in geom_nodes {
        let g_tag = g_el.name().to_string();
        match g_tag.as_str() {
            "rect" => {
                let x = get_attr_expr(g_el, "x").ok_or_else(|| {
                    syn::Error::new(g_el.name().span(), "Attribute 'x' is required for <rect>")
                })?;
                let y = get_attr_expr(g_el, "y").ok_or_else(|| {
                    syn::Error::new(g_el.name().span(), "Attribute 'y' is required for <rect>")
                })?;
                let w = get_attr_expr(g_el, "w").ok_or_else(|| {
                    syn::Error::new(g_el.name().span(), "Attribute 'w' is required for <rect>")
                })?;
                let h = get_attr_expr(g_el, "h").ok_or_else(|| {
                    syn::Error::new(g_el.name().span(), "Attribute 'h' is required for <rect>")
                })?;
                let color = get_attr_expr(g_el, "color").ok_or_else(|| {
                    syn::Error::new(
                        g_el.name().span(),
                        "Attribute 'color' is required for <rect>",
                    )
                })?;
                geom_tokens.push(quote! {
                    _horb = _horb.rect(#x, #y, #w, #h, #color);
                });
            }
            "teleport-point" => {
                let x = get_attr_expr(g_el, "x").ok_or_else(|| {
                    syn::Error::new(
                        g_el.name().span(),
                        "Attribute 'x' is required for <teleport-point>",
                    )
                })?;
                let y = get_attr_expr(g_el, "y").ok_or_else(|| {
                    syn::Error::new(
                        g_el.name().span(),
                        "Attribute 'y' is required for <teleport-point>",
                    )
                })?;
                let action = get_attr_expr(g_el, "action").ok_or_else(|| {
                    syn::Error::new(
                        g_el.name().span(),
                        "Attribute 'action' is required for <teleport-point>",
                    )
                })?;
                geom_tokens.push(quote! {
                    _horb = _horb.teleport_point(#x, #y, #action);
                });
            }
            _ => {
                return Err(syn::Error::new(
                    g_el.name().span(),
                    format!("Unknown canvas element <{g_tag}>"),
                ));
            }
        }
    }
    Ok(quote! {
        #(#geom_tokens)*
    })
}

fn expand_node(node: &Node) -> Result<proc_macro2::TokenStream, syn::Error> {
    match node {
        Node::Element(el) => {
            let tag_name = el.name().to_string();
            match tag_name.as_str() {
                "window" => expand_window(el),
                "text" => {
                    let val = get_single_child_or_value(el)?;
                    Ok(quote! {
                        _horb = _horb.text(#val);
                    })
                }
                "tabs" => expand_tabs(el),
                "buttons" => expand_buttons(el),
                "list" => expand_list(el),
                "form" => expand_form(el),
                "canvas" => expand_canvas(el),
                _ => Err(syn::Error::new(
                    el.name().span(),
                    format!("Unknown element <{tag_name}>"),
                )),
            }
        }
        Node::Text(t) => {
            let s = t.value_string();
            Ok(quote! {
                _horb = _horb.text(#s);
            })
        }
        Node::Block(b) => Ok(quote! {
            _horb = _horb.text(#b);
        }),
        _ => Ok(quote! {}),
    }
}
