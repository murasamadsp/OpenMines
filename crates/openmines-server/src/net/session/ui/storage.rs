use crate::game::logic::horb::{Button, Horb};

pub fn render(view: &crate::game::StorageGuiView) -> Vec<u8> {
    Horb::new("Склад")
        .crystals(" ", " ", false, view.crystal_lines.clone())
        .button(Button::new("Передать", "transfer:%M%"))
        .button(Button::new(
            "Удалить",
            format!("pack_op:remove:{}:{}", view.x, view.y),
        ))
        .close_button()
        .payload()
}
