pub fn render(view: &crate::game::SpotGuiView) -> Vec<u8> {
    use super::horb::gui;
    gui! {
        <window title="СПОТ">
            <buttons>
                <button label="Удалить" action=format!("pack_op:remove:{}:{}", view.x, view.y) />
            </buttons>
        </window>
    }
    .close_button()
    .payload()
}
