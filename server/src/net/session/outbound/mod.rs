//! Исходящие «тонкие» отправки пакетов: только форматирование и `send_u_packet`.
//! Зависит от [`super::prelude`] и не тянет `social` / `ui` / обработчики событий.

pub mod chat_sync;
pub mod inventory_sync;
pub mod player_sync;
