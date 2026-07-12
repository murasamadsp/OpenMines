use crate::net::session::prelude::*;

pub(super) fn apply(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, data: &str) {
    use crate::game::logic::settings::SettingsSaveError;

    match crate::game::logic::settings::save_settings(state, pid, data) {
        Ok(wire) => {
            send_u_packet(tx, "#S", &wire);
            open(state, tx, pid);
        }
        Err(SettingsSaveError::MalformedPayload) => {
            tracing::warn!(player_id = %pid, payload = data, "Malformed settings payload");
            error(tx, "Некорректный формат настроек.");
        }
        Err(SettingsSaveError::InvalidInteger("isca")) => {
            tracing::warn!(player_id = %pid, "Invalid isca setting");
            error(tx, "Некорректный масштаб интерфейса.");
        }
        Err(SettingsSaveError::InvalidInteger("tsca")) => {
            tracing::warn!(player_id = %pid, "Invalid tsca setting");
            error(tx, "Некорректный масштаб территории.");
        }
        Err(SettingsSaveError::InvalidInteger(key)) => {
            tracing::warn!(player_id = %pid, key, "Invalid integer setting");
            error(tx, "Некорректное значение настройки.");
        }
        Err(SettingsSaveError::InvalidBool(key)) => {
            tracing::warn!(player_id = %pid, key, "Invalid bool setting");
            error(tx, "Некорректное значение настройки.");
        }
        Err(SettingsSaveError::MissingState) => {
            tracing::error!(player_id = %pid, "Player settings state missing for save");
            error(tx, "Состояние настроек недоступно.");
        }
    }
}

pub fn open(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    use super::horb::{Button, Horb, RichRow, Tab};

    let Some(view) = crate::game::logic::settings::settings_view(state, pid) else {
        return;
    };
    let settings = view.settings;
    let scale_values = "0:мелко#1:КРУПНО#";
    let mut window = Horb::new("Настройки")
        .tab(Tab::active("Настройки"))
        .rich_row(RichRow::dropdown(
            "Масштаб интерфейса",
            scale_values,
            "isca",
            i64::from(settings.isca),
        ))
        .rich_row(RichRow::dropdown(
            "Масштаб территории",
            scale_values,
            "tsca",
            i64::from(settings.tsca),
        ))
        .rich_row(RichRow::toggle(
            "Включить управление мышкой",
            "mous",
            settings.mous,
        ))
        .rich_row(RichRow::toggle(
            "Упрощённый режим графики",
            "pot",
            settings.pot,
        ))
        .rich_row(RichRow::toggle(
            "Принудительно обновлять породы (увеличит потр. CPU)",
            "frc",
            settings.frc,
        ))
        .rich_row(RichRow::toggle(
            "CTRL переключает скорость робота (вместо удерживания)",
            "ctrl",
            settings.ctrl,
        ))
        .rich_row(RichRow::toggle(
            "Отключить ближайшие звуки",
            "mof",
            settings.mof,
        ))
        .button(Button::new("Сохранить", "save:%R%"));
    if !view.has_clan {
        window = window.button(Button::new("Создать клан", "clancreate"));
    }
    window
        .button(Button::new("Выйти", "exit"))
        .send(state, tx, pid, "settings");
}

fn error(tx: &Outbox, message: &str) {
    send_u_packet(tx, "OK", &ok_message("НАСТРОЙКИ", message).1);
}
