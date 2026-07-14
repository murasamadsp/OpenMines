use proc_macro::TokenStream;
use quote::quote;
use rstml::node::{Node, NodeAttribute, NodeElement, NodeName};
use rstml::parse2;
use syn::spanned::Spanned;

#[proc_macro]
pub fn gui(input: TokenStream) -> TokenStream {
    let tokens = proc_macro2::TokenStream::from(input);
    match parse2(tokens) {
        Ok(nodes) if nodes.len() == 1 => match expand_root(&nodes[0]) {
            Ok(expanded) => expanded.into(),
            Err(error) => error.to_compile_error().into(),
        },
        Ok(_) => syn::Error::new(
            proc_macro2::Span::call_site(),
            "Expected exactly one root element <window>",
        )
        .to_compile_error()
        .into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn name_is(name: &NodeName, expected: &str) -> bool {
    match name {
        NodeName::Path(path) => path.path.is_ident(expected),
        NodeName::Punctuated(_) => expected.contains('-') && name.to_string() == expected,
        NodeName::Block(_) => false,
    }
}

fn validate_attributes(el: &NodeElement, allowed: &[&str]) -> Result<(), syn::Error> {
    let attributes = el.attributes();
    for (index, attribute) in attributes.iter().enumerate() {
        match attribute {
            NodeAttribute::Attribute(keyed) => {
                let Some(name) = allowed
                    .iter()
                    .copied()
                    .find(|name| name_is(&keyed.key, name))
                else {
                    return Err(syn::Error::new(
                        keyed.key.span(),
                        format!("Unsupported attribute '{}' for <{}>", keyed.key, el.name()),
                    ));
                };
                if attributes[..index].iter().any(|previous| {
                    matches!(previous, NodeAttribute::Attribute(previous) if name_is(&previous.key, name))
                }) {
                    return Err(syn::Error::new(
                        keyed.key.span(),
                        format!("Duplicate attribute '{name}' for <{}>", el.name()),
                    ));
                }
            }
            NodeAttribute::Block(_) => {
                return Err(syn::Error::new(
                    attribute.span(),
                    format!("Unsupported attribute syntax for <{}>", el.name()),
                ));
            }
        }
    }
    Ok(())
}

fn get_attr_expr(el: &NodeElement, name: &str) -> Option<proc_macro2::TokenStream> {
    el.attributes()
        .iter()
        .find_map(|attribute| match attribute {
            NodeAttribute::Attribute(keyed) if name_is(&keyed.key, name) => {
                keyed.value().map(|value| quote! { #value })
            }
            _ => None,
        })
}

fn required_attr(el: &NodeElement, name: &str) -> Result<proc_macro2::TokenStream, syn::Error> {
    get_attr_expr(el, name).ok_or_else(|| {
        syn::Error::new(
            el.name().span(),
            format!("Attribute '{name}' is required for <{}>", el.name()),
        )
    })
}

fn literal_attr(el: &NodeElement, name: &str) -> Option<String> {
    let expression = el
        .attributes()
        .iter()
        .find_map(|attribute| match attribute {
            NodeAttribute::Attribute(keyed) if name_is(&keyed.key, name) => keyed.value(),
            _ => None,
        })?;
    let syn::Expr::Lit(literal) = expression else {
        return None;
    };
    match &literal.lit {
        syn::Lit::Str(value) => Some(value.value()),
        syn::Lit::Int(value) => Some(value.base10_digits().to_owned()),
        syn::Lit::Float(value) => Some(value.base10_digits().to_owned()),
        syn::Lit::Bool(value) => Some(value.value.to_string()),
        syn::Lit::Char(value) => Some(value.value().to_string()),
        syn::Lit::Byte(value) => Some(value.value().to_string()),
        _ => None,
    }
}

fn validate_leaf(el: &NodeElement) -> Result<(), syn::Error> {
    for child in &el.children {
        if !matches!(child, Node::Text(text) if text.value_string().trim().is_empty()) {
            return Err(syn::Error::new(
                child.span(),
                format!("<{}> cannot have children", el.name()),
            ));
        }
    }
    Ok(())
}

fn get_single_child_or_value(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    match el.children.as_slice() {
        [] => Ok(quote! { "" }),
        [Node::Text(text)] => {
            let value = text.value_string();
            Ok(quote! { #value })
        }
        [Node::Block(block)] => Ok(quote! { #block }),
        children => {
            let mut format_string = String::new();
            let mut expressions = Vec::with_capacity(children.len());
            for child in children {
                match child {
                    Node::Text(text) => {
                        format_string.push_str("{}");
                        let value = text.value_string();
                        expressions.push(quote! { #value });
                    }
                    Node::Block(block) => {
                        format_string.push_str("{}");
                        expressions.push(quote! { #block });
                    }
                    _ => {
                        return Err(syn::Error::new(
                            child.span(),
                            "Expected text or expression inside element",
                        ));
                    }
                }
            }
            Ok(quote! { format!(#format_string, #(#expressions),*) })
        }
    }
}

fn expand_window(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(el, &["title", "style"])?;
    let title = required_attr(el, "title")?;
    let css = get_attr_expr(el, "style").map(|style| quote! { .css(#style) });
    let children = el
        .children
        .iter()
        .map(expand_node)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(quote! {
        {
            crate::net::session::ui::horb::Horb::new(#title)
            #css
            #(#children)*
        }
    })
}

fn expand_tabs(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(el, &[])?;
    let mut tabs = Vec::new();
    for child in &el.children {
        match child {
            Node::Element(tab) if name_is(tab.name(), "tab") => {
                validate_attributes(tab, &["label", "action", "active"])?;
                validate_leaf(tab)?;
                let label = required_attr(tab, "label")?;
                let action = get_attr_expr(tab, "action").unwrap_or_else(|| quote! { "" });
                let active = get_attr_expr(tab, "active").unwrap_or_else(|| quote! { false });
                tabs.push(quote! {
                    .tab(if #active {
                        crate::net::session::ui::horb::Tab::active(#label)
                    } else {
                        crate::net::session::ui::horb::Tab::new(#label, #action)
                    })
                });
            }
            Node::Element(other) => {
                return Err(syn::Error::new(
                    other.name().span(),
                    format!(
                        "Only <tab> is allowed inside <tabs>, found <{}>",
                        other.name()
                    ),
                ));
            }
            Node::Text(text) if text.value_string().trim().is_empty() => {}
            _ => {
                return Err(syn::Error::new(
                    child.span(),
                    "Only <tab> elements are allowed inside <tabs>",
                ));
            }
        }
    }
    Ok(quote! { #(#tabs)* })
}

fn expand_buttons(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(el, &[])?;
    let mut buttons = Vec::new();
    for child in &el.children {
        match child {
            Node::Element(button) if name_is(button.name(), "button") => {
                validate_attributes(button, &["label", "action"])?;
                validate_leaf(button)?;
                let label = required_attr(button, "label")?;
                let action = required_attr(button, "action")?;
                buttons.push(quote! {
                    .button(crate::net::session::ui::horb::Button::new(#label, #action))
                });
            }
            Node::Element(other) => {
                return Err(syn::Error::new(
                    other.name().span(),
                    format!(
                        "Only <button> is allowed inside <buttons>, found <{}>",
                        other.name()
                    ),
                ));
            }
            Node::Text(text) if text.value_string().trim().is_empty() => {}
            _ => {
                return Err(syn::Error::new(
                    child.span(),
                    "Only <button> elements are allowed inside <buttons>",
                ));
            }
        }
    }
    Ok(quote! { #(#buttons)* })
}

fn expand_list(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(el, &[])?;
    let mut rows = Vec::new();
    for child in &el.children {
        match child {
            Node::Element(row) if name_is(row.name(), "row") => {
                validate_attributes(row, &["title", "subtitle", "action"])?;
                validate_leaf(row)?;
                let title = required_attr(row, "title")?;
                let subtitle = get_attr_expr(row, "subtitle").unwrap_or_else(|| quote! { "" });
                let action = get_attr_expr(row, "action").unwrap_or_else(|| quote! { "" });
                rows.push(quote! {
                    .list_row(crate::net::session::ui::ListRow::new(#title, #subtitle, #action))
                });
            }
            Node::Element(other) => {
                return Err(syn::Error::new(
                    other.name().span(),
                    format!(
                        "Only <row> is allowed inside <list>, found <{}>",
                        other.name()
                    ),
                ));
            }
            Node::Text(text) if text.value_string().trim().is_empty() => {}
            _ => {
                return Err(syn::Error::new(
                    child.span(),
                    "Only <row> elements are allowed inside <list>",
                ));
            }
        }
    }
    Ok(quote! { #(#rows)* })
}

fn expand_form_row(row: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    if name_is(row.name(), "text-row") {
        validate_attributes(row, &["label"])?;
        validate_leaf(row)?;
        let label = required_attr(row, "label")?;
        Ok(quote! { .rich_row(crate::net::session::ui::horb::RichRow::text(#label)) })
    } else if name_is(row.name(), "toggle-row") {
        validate_attributes(row, &["label", "key", "active"])?;
        validate_leaf(row)?;
        let label = required_attr(row, "label")?;
        let key = required_attr(row, "key")?;
        let active = get_attr_expr(row, "active").unwrap_or_else(|| quote! { false });
        Ok(
            quote! { .rich_row(crate::net::session::ui::horb::RichRow::toggle(#label, #key, #active)) },
        )
    } else if name_is(row.name(), "uint-row") {
        validate_attributes(row, &["label", "key", "value"])?;
        validate_leaf(row)?;
        let label = required_attr(row, "label")?;
        let key = required_attr(row, "key")?;
        let value = get_attr_expr(row, "value").unwrap_or_else(|| quote! { 0 });
        Ok(quote! { .rich_row(crate::net::session::ui::horb::RichRow::uint(#label, #key, #value)) })
    } else if name_is(row.name(), "button-row") {
        validate_attributes(row, &["label", "btn-label", "action"])?;
        validate_leaf(row)?;
        let label = required_attr(row, "label")?;
        let button_label = required_attr(row, "btn-label")?;
        let action = required_attr(row, "action")?;
        Ok(
            quote! { .rich_row(crate::net::session::ui::horb::RichRow::button(#label, #button_label, #action)) },
        )
    } else if name_is(row.name(), "dropdown-row") {
        expand_dropdown_row(row)
    } else {
        Err(syn::Error::new(
            row.name().span(),
            format!("Unknown form element <{}>", row.name()),
        ))
    }
}

fn expand_dropdown_row(row: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(row, &["label", "key", "selected"])?;
    let label = required_attr(row, "label")?;
    let key = required_attr(row, "key")?;
    let selected = get_attr_expr(row, "selected").unwrap_or_else(|| quote! { 0 });
    let mut options = Vec::new();
    let mut literal_options = Vec::new();
    let mut all_options_are_literal = true;

    for child in &row.children {
        match child {
            Node::Element(option) if name_is(option.name(), "option") => {
                validate_attributes(option, &["value", "label"])?;
                validate_leaf(option)?;
                options.push((
                    required_attr(option, "value")?,
                    required_attr(option, "label")?,
                ));
                match (literal_attr(option, "value"), literal_attr(option, "label")) {
                    (Some(value), Some(label)) => literal_options.push((value, label)),
                    _ => all_options_are_literal = false,
                }
            }
            Node::Element(other) => {
                return Err(syn::Error::new(
                    other.name().span(),
                    format!(
                        "Only <option> is allowed inside <dropdown-row>, found <{}>",
                        other.name()
                    ),
                ));
            }
            Node::Text(text) if text.value_string().trim().is_empty() => {}
            _ => {
                return Err(syn::Error::new(
                    child.span(),
                    "Only <option> elements are allowed inside <dropdown-row>",
                ));
            }
        }
    }

    let options = if all_options_are_literal {
        let value = literal_options
            .into_iter()
            .map(|(value, label)| format!("{value}:{label}"))
            .collect::<Vec<_>>()
            .join("#");
        let value = if value.is_empty() {
            value
        } else {
            format!("{value}#")
        };
        quote! { #value }
    } else {
        let format_string = "{}:{}#".repeat(options.len());
        let expressions = options
            .iter()
            .flat_map(|(value, label)| [quote! { #value }, quote! { #label }]);
        quote! { format!(#format_string, #(#expressions),*) }
    };

    Ok(quote! {
        .rich_row(crate::net::session::ui::horb::RichRow::dropdown(#label, #options, #key, #selected))
    })
}

fn expand_form(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(el, &[])?;
    let mut rows = Vec::new();
    for child in &el.children {
        match child {
            Node::Element(row) => rows.push(expand_form_row(row)?),
            Node::Text(text) if text.value_string().trim().is_empty() => {}
            _ => {
                return Err(syn::Error::new(
                    child.span(),
                    "Only form row elements are allowed inside <form>",
                ));
            }
        }
    }
    Ok(quote! { #(#rows)* })
}

fn expand_canvas(el: &NodeElement) -> Result<proc_macro2::TokenStream, syn::Error> {
    validate_attributes(el, &["style"])?;
    let css = get_attr_expr(el, "style").map(|style| quote! { .css(#style) });
    let mut geometry = Vec::new();
    for child in &el.children {
        match child {
            Node::Element(rect) if name_is(rect.name(), "rect") => {
                validate_attributes(rect, &["x", "y", "w", "h", "color"])?;
                validate_leaf(rect)?;
                let x = required_attr(rect, "x")?;
                let y = required_attr(rect, "y")?;
                let width = required_attr(rect, "w")?;
                let height = required_attr(rect, "h")?;
                let color = required_attr(rect, "color")?;
                geometry.push(quote! { .rect(#x, #y, #width, #height, #color) });
            }
            Node::Element(point) if name_is(point.name(), "teleport-point") => {
                validate_attributes(point, &["x", "y", "action"])?;
                validate_leaf(point)?;
                let x = required_attr(point, "x")?;
                let y = required_attr(point, "y")?;
                let action = required_attr(point, "action")?;
                geometry.push(quote! { .teleport_point(#x, #y, #action) });
            }
            Node::Element(other) => {
                return Err(syn::Error::new(
                    other.name().span(),
                    format!("Unknown canvas element <{}>", other.name()),
                ));
            }
            Node::Text(text) if text.value_string().trim().is_empty() => {}
            _ => {
                return Err(syn::Error::new(
                    child.span(),
                    "Only canvas elements (<rect>, <teleport-point>) are allowed inside <canvas>",
                ));
            }
        }
    }
    Ok(quote! { #css #(#geometry)* })
}

fn expand_root(node: &Node) -> Result<proc_macro2::TokenStream, syn::Error> {
    match node {
        Node::Element(element) if name_is(element.name(), "window") => expand_window(element),
        Node::Element(element) => Err(syn::Error::new(
            element.name().span(),
            "Expected root element <window>",
        )),
        _ => Err(syn::Error::new(
            node.span(),
            "Expected root element <window>",
        )),
    }
}

fn expand_node(node: &Node) -> Result<proc_macro2::TokenStream, syn::Error> {
    match node {
        Node::Element(element) if name_is(element.name(), "text") => {
            validate_attributes(element, &[])?;
            let value = get_single_child_or_value(element)?;
            Ok(quote! { .text(#value) })
        }
        Node::Element(element) if name_is(element.name(), "tabs") => expand_tabs(element),
        Node::Element(element) if name_is(element.name(), "buttons") => expand_buttons(element),
        Node::Element(element) if name_is(element.name(), "list") => expand_list(element),
        Node::Element(element) if name_is(element.name(), "form") => expand_form(element),
        Node::Element(element) if name_is(element.name(), "canvas") => expand_canvas(element),
        Node::Element(element) => Err(syn::Error::new(
            element.name().span(),
            format!("Unknown element <{}>", element.name()),
        )),
        Node::Text(text) => {
            let value = text.value_string();
            Ok(quote! { .text(#value) })
        }
        Node::Block(block) => Ok(quote! { .text(#block) }),
        _ => Ok(quote! {}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: proc_macro2::TokenStream) -> NodeElement {
        let nodes = parse2(input).expect("valid markup");
        match nodes.into_iter().next() {
            Some(Node::Element(element)) => element,
            _ => panic!("expected element"),
        }
    }

    #[test]
    fn builds_hygienic_chain_without_internal_binding() {
        let window = parse(quote! {
            <window title="Title"><buttons><button label="OK" action="ok" /></buttons></window>
        });
        let generated = expand_window(&window).expect("expands").to_string();
        assert!(generated.contains("Horb :: new"));
        assert!(generated.contains(". button"));
        assert!(!generated.contains("_horb"));
    }

    #[test]
    fn folds_literal_dropdown_options_at_compile_time() {
        let row = parse(quote! {
            <dropdown-row label="Rank" key="rank"><option value=0 label="Member" /><option value=1 label="Leader" /></dropdown-row>
        });
        let generated = expand_form_row(&row).expect("expands").to_string();
        assert!(generated.contains("0:Member#1:Leader#"));
        assert!(!generated.contains("vec !"));
        assert!(!generated.contains("join"));
    }

    #[test]
    fn dynamic_dropdown_uses_one_format_call() {
        let row = parse(quote! {
            <dropdown-row label="Rank" key="rank"><option value=rank label=name /></dropdown-row>
        });
        let generated = expand_form_row(&row).expect("expands").to_string();
        assert!(generated.contains("format !"));
        assert!(!generated.contains("vec !"));
        assert!(!generated.contains("join"));
    }

    #[test]
    fn rejects_unknown_attribute_and_invalid_form_content() {
        let window = parse(quote! { <window title="Title" typo="x" /> });
        assert!(
            expand_window(&window)
                .expect_err("must reject typo")
                .to_string()
                .contains("Unsupported attribute 'typo'")
        );

        let form = parse(quote! { <form>unexpected</form> });
        assert!(
            expand_form(&form)
                .expect_err("must reject text")
                .to_string()
                .contains("Only form row elements")
        );
    }

    #[test]
    fn rejects_duplicate_attributes_and_non_window_roots() {
        let window = parse(quote! { <window title="One" title="Two" /> });
        assert!(
            expand_window(&window)
                .expect_err("must reject duplicate")
                .to_string()
                .contains("Duplicate attribute 'title'")
        );

        let tabs = parse(quote! { <tabs /> });
        assert!(
            expand_root(&Node::Element(tabs))
                .expect_err("must require window root")
                .to_string()
                .contains("Expected root element <window>")
        );
    }

    #[test]
    fn rejects_content_inside_leaf_elements() {
        assert!(
            expand_buttons(&parse(
                quote! { <buttons><button label="OK" action="ok"><text /></button></buttons> }
            ))
            .expect_err("must reject button children")
            .to_string()
            .contains("<button> cannot have children")
        );

        assert!(
            expand_dropdown_row(&parse(quote! { <dropdown-row label="Rank" key="rank"><option value=1 label="One">bad</option></dropdown-row> }))
                .expect_err("must reject option content")
                .to_string()
                .contains("<option> cannot have children")
        );
    }
}
