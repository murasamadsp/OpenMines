//! Async dispatch for legacy GUI handlers.

use std::sync::Arc;

use crate::game::GameState;

#[derive(Clone, Copy)]
enum GuiAsyncHandler {
    Auction,
    Other,
}

pub fn spawn_gui_async_task(
    state: &Arc<GameState>,
    tx: crate::net::session::outbox::Outbox,
    player_id: crate::game::PlayerId,
    button: String,
) {
    let handler = if crate::game::logic::gui::gui_buttons::is_auction_button(&button) {
        GuiAsyncHandler::Auction
    } else {
        GuiAsyncHandler::Other
    };
    let task_name = match handler {
        GuiAsyncHandler::Auction => "auction_gui",
        GuiAsyncHandler::Other => "other_gui_button",
    };
    let task_state = state.clone();
    super::spawn_session_async_task(state, task_name, async move {
        match handler {
            GuiAsyncHandler::Auction => {
                crate::game::logic::gui::gui_buttons::handle_auction_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
            GuiAsyncHandler::Other => {
                crate::game::logic::gui::gui_buttons::handle_gui_button(
                    &task_state,
                    &tx,
                    player_id,
                    &button,
                )
                .await;
            }
        }
    });
}
