//! Legacy GUI documents and their byte-exact `GU` payloads.
//!
//! This module has no server, ECS, session, or transport dependency. Simulation
//! creates immutable screen views, presentation renders them into these documents,
//! and the session owner delivers the resulting payload.

/// A client GUI document carried by the `GU` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuiDocument {
    Horb(Horb),
}

impl GuiDocument {
    #[must_use]
    pub fn payload(&self) -> Vec<u8> {
        match self {
            Self::Horb(window) => window.payload(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    label: String,
    action: String,
}

impl Button {
    #[must_use]
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    label: String,
    action: String,
}

impl Tab {
    #[must_use]
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
        }
    }

    #[must_use]
    pub fn active(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    title: String,
    subtitle: String,
    action: String,
}

impl ListRow {
    #[must_use]
    pub fn new(
        title: impl Into<String>,
        subtitle: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: subtitle.into(),
            action: action.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichRow {
    label: String,
    kind: String,
    values: String,
    action: String,
    value: String,
}

impl RichRow {
    #[must_use]
    pub fn fill(label: impl Into<String>, values: impl Into<String>) -> Self {
        Self::new(label, "fill", values, "", "")
    }

    #[must_use]
    pub fn text(label: impl Into<String>) -> Self {
        Self::new(label, "text", "", "", "")
    }

    #[must_use]
    pub fn toggle(label: impl Into<String>, key: impl Into<String>, on: bool) -> Self {
        Self::new(label, "bool", "", key, if on { "1" } else { "0" })
    }

    #[must_use]
    pub fn uint(label: impl Into<String>, key: impl Into<String>, default: i64) -> Self {
        Self::new(label, "uint", "", key, default.to_string())
    }

    #[must_use]
    pub fn dropdown(
        label: impl Into<String>,
        values: impl Into<String>,
        key: impl Into<String>,
        selected: i64,
    ) -> Self {
        Self::new(label, "drop", values, key, selected.to_string())
    }

    #[must_use]
    pub fn button(
        label: impl Into<String>,
        button_label: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self::new(label, "button", button_label, action, "")
    }

    fn new(
        label: impl Into<String>,
        kind: impl Into<String>,
        values: impl Into<String>,
        action: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            kind: kind.into(),
            values: values.into(),
            action: action.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CanvasElement {
    Rect {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: String,
    },
    TeleportPoint {
        x: i32,
        y: i32,
        action: String,
    },
}

impl CanvasElement {
    fn extend_flattened(&self, output: &mut Vec<String>) {
        match self {
            Self::Rect {
                x,
                y,
                width,
                height,
                color,
            } => output.push(format!("{x}X{y}Y{width}w{height}h=R#{color}")),
            Self::TeleportPoint { x, y, action } => {
                output.push(format!("{x}X{y}Y=t"));
                output.push(action.clone());
            }
        }
    }
}

/// A HORB window. Its serialization is the legacy Unity `HORBConfig` contract.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Horb {
    title: String,
    text: String,
    buttons: Vec<Button>,
    tabs: Vec<Tab>,
    list: Vec<ListRow>,
    rich_list: Vec<RichRow>,
    admin: bool,
    crystal_lines: Vec<String>,
    crystal_left: String,
    crystal_right: String,
    crystal_buy: bool,
    canvas: Vec<CanvasElement>,
    css: String,
    card: String,
    inventory: String,
    input_placeholder: String,
    input_console: bool,
}

impl Horb {
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn css(mut self, css: impl Into<String>) -> Self {
        self.css = css.into();
        self
    }

    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    #[must_use]
    pub fn input(mut self, placeholder: impl Into<String>, focus_console: bool) -> Self {
        self.input_placeholder = placeholder.into();
        self.input_console = focus_console;
        self
    }

    #[must_use]
    pub fn card(mut self, card: impl Into<String>) -> Self {
        self.card = card.into();
        self
    }

    #[must_use]
    pub fn inventory(mut self, inventory: impl Into<String>) -> Self {
        self.inventory = inventory.into();
        self
    }

    #[must_use]
    pub const fn admin(mut self, admin: bool) -> Self {
        self.admin = admin;
        self
    }

    #[must_use]
    pub fn button(mut self, button: Button) -> Self {
        self.buttons.push(button);
        self
    }

    #[must_use]
    pub fn close_button(self) -> Self {
        self.button(Button::new("ВЫЙТИ", "exit"))
    }

    #[must_use]
    pub fn tab(mut self, tab: Tab) -> Self {
        self.tabs.push(tab);
        self
    }

    #[must_use]
    pub fn list_row(mut self, row: ListRow) -> Self {
        self.list.push(row);
        self
    }

    #[must_use]
    pub fn rich_row(mut self, row: RichRow) -> Self {
        self.rich_list.push(row);
        self
    }

    #[must_use]
    pub fn crystals(
        mut self,
        left: impl Into<String>,
        right: impl Into<String>,
        buy: bool,
        lines: Vec<String>,
    ) -> Self {
        self.crystal_left = left.into();
        self.crystal_right = right.into();
        self.crystal_buy = buy;
        self.crystal_lines = lines;
        self
    }

    #[must_use]
    pub fn rect(
        mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: impl Into<String>,
    ) -> Self {
        self.canvas.push(CanvasElement::Rect {
            x,
            y,
            width,
            height,
            color: color.into(),
        });
        self
    }

    #[must_use]
    pub fn teleport_point(mut self, x: i32, y: i32, action: impl Into<String>) -> Self {
        self.canvas.push(CanvasElement::TeleportPoint {
            x,
            y,
            action: action.into(),
        });
        self
    }

    #[must_use]
    pub fn minimap(
        mut self,
        center_x: i32,
        center_y: i32,
        radius: i32,
        cell_empty: impl Fn(i32, i32) -> Option<bool>,
        markers: &[(i32, i32, String)],
    ) -> Self {
        const PIXELS: i32 = 18;
        let center_chunk = (center_x.div_euclid(32), center_y.div_euclid(32));
        for y_offset in -radius..=radius {
            for x_offset in -radius..=radius {
                let (x, y) = (
                    (center_chunk.0 + x_offset) * 32 + 16,
                    (center_chunk.1 + y_offset) * 32 + 16,
                );
                let Some(empty) = cell_empty(x, y) else {
                    continue;
                };
                let color = if empty { "008000" } else { "6495ed" };
                self = self.rect(x_offset * PIXELS, -y_offset * PIXELS, PIXELS, PIXELS, color);
            }
        }
        self = self.rect(0, 0, PIXELS, PIXELS, "ff3030");
        for (x, y, action) in markers {
            let offset = (
                x.div_euclid(32) - center_chunk.0,
                y.div_euclid(32) - center_chunk.1,
            );
            if offset.0.abs() <= radius && offset.1.abs() <= radius {
                self = self.teleport_point(offset.0 * PIXELS, -offset.1 * PIXELS, action);
            }
        }
        let side = (2 * radius + 2) * PIXELS;
        self.css(format!("canv-w={side};canv-h={side}"))
    }

    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        object.insert("title".into(), self.title.clone().into());
        object.insert("text".into(), self.text.clone().into());
        object.insert("back".into(), false.into());
        object.insert("admin".into(), self.admin.into());
        insert_non_empty(&mut object, "css", &self.css);
        insert_non_empty(&mut object, "card", &self.card);
        insert_non_empty(&mut object, "inv", &self.inventory);
        insert_non_empty(&mut object, "input_place", &self.input_placeholder);
        if self.input_console {
            object.insert("input_console".into(), true.into());
        }
        if !self.canvas.is_empty() {
            let mut canvas = Vec::new();
            for element in &self.canvas {
                element.extend_flattened(&mut canvas);
            }
            object.insert("canvas".into(), canvas.into());
        }
        if !self.buttons.is_empty() {
            let mut buttons: Vec<String> = self
                .buttons
                .iter()
                .flat_map(|button| [button.label.clone(), button.action.clone()])
                .collect();
            if buttons.last().map(String::as_str) != Some("exit") {
                buttons.extend(["ВЫЙТИ".into(), "exit".into()]);
            }
            object.insert("buttons".into(), buttons.into());
        }
        if !self.tabs.is_empty() {
            object.insert(
                "tabs".into(),
                self.tabs
                    .iter()
                    .flat_map(|tab| [tab.label.clone(), tab.action.clone()])
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        if !self.crystal_lines.is_empty() {
            object.insert("crys_left".into(), self.crystal_left.clone().into());
            object.insert("crys_right".into(), self.crystal_right.clone().into());
            object.insert("crys_lines".into(), self.crystal_lines.clone().into());
            if self.crystal_buy {
                object.insert("crys_buy".into(), true.into());
            }
        }
        if !self.list.is_empty() {
            object.insert(
                "list".into(),
                self.list
                    .iter()
                    .flat_map(|row| [row.title.clone(), row.subtitle.clone(), row.action.clone()])
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        if !self.rich_list.is_empty() {
            object.insert(
                "richList".into(),
                self.rich_list
                    .iter()
                    .flat_map(|row| {
                        [
                            row.label.clone(),
                            row.kind.clone(),
                            row.values.clone(),
                            row.action.clone(),
                            row.value.clone(),
                        ]
                    })
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        serde_json::Value::Object(object)
    }

    #[must_use]
    pub fn payload(&self) -> Vec<u8> {
        format!("horb:{}", self.to_json()).into_bytes()
    }
}

fn insert_non_empty(
    object: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: &str,
) {
    if !value.is_empty() {
        object.insert(key.into(), value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, GuiDocument, Horb, ListRow, RichRow, Tab};

    #[test]
    fn horb_payload_preserves_legacy_flat_collections() {
        let window = Horb::new("Window")
            .tab(Tab::active("Main"))
            .tab(Tab::new("Other", "other"))
            .list_row(ListRow::new("Row", "Sub", "open"))
            .rich_row(RichRow::toggle("Enabled", "enabled", true))
            .button(Button::new("OK", "ok"));
        let json = window.to_json();
        assert_eq!(
            json["tabs"],
            serde_json::json!(["Main", "", "Other", "other"])
        );
        assert_eq!(json["list"], serde_json::json!(["Row", "Sub", "open"]));
        assert_eq!(
            json["richList"],
            serde_json::json!(["Enabled", "bool", "", "enabled", "1"])
        );
        assert_eq!(
            json["buttons"],
            serde_json::json!(["OK", "ok", "ВЫЙТИ", "exit"])
        );
        assert!(GuiDocument::Horb(window).payload().starts_with(b"horb:{"));
    }
}
