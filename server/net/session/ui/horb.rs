//! Единый builder HORB-окон (GU `"horb:{json}"`).
//!
//! Зеркало C# `HORB`/`Window`/`Page`/`RichList`, но под РЕАЛЬНЫЙ контракт клиента
//! (`client/Assets/Scripts/Data/HORBConfig.cs` + `PopupManager.ShowHORB`). Клиент
//! парсит payload как `JsonUtility.FromJson<HORBConfig>` — структура ПЛОСКАЯ, все
//! коллекции — `string[]` фиксированных кортежей:
//!
//! - `buttons` — пары `[label, action, …]`; клиент берёт action по `buttons[2n+1]`.
//!   **Массив ОБЯЗАН быть чётным** — иначе NRE `PopupManager.cs:222`. Типы здесь
//!   это гарантируют (каждая кнопка = ровно пара).
//! - `list` — тройки `[title, subtitle, action, …]`; action по `list[3n+2]`; рендерится
//!   в `ScrollRect` (`scrollView`) → длинные списки прокручиваются (фикс «за экран»).
//! - `richList` — 5-кортежи `[label, kind, values, action, value, …]`.
//!
//! Раньше каждый хендлер лепил JSON руками (gun GUI вообще слал `tabs:[{объект}]` —
//! а `HORBConfig.tabs` это `string[]`, `JsonUtility` это не парсит → окно не
//! открывалось). Этот модуль централизует сборку и устраняет такие расхождения.
//!
//! По мере миграции окон сюда добавятся `tabs`/`css`/`back` (нужны для market и пр.).

use crate::net::session::prelude::*;

/// Кнопка окна: подпись + action (уходит серверу при клике).
pub struct Button {
    label: String,
    action: String,
}

impl Button {
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
        }
    }
}

/// Строка списка `list`: заголовок + подзаголовок + action.
/// Пустой `subtitle` → клиент скрывает кнопку строки (не-кликабельно).
pub struct ListRow {
    title: String,
    subtitle: String,
    action: String,
}

impl ListRow {
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

/// Запись `richList` (5-кортеж: label, kind, values, action, value).
/// `kind` — тип строки (`"fill"`, `"text"`, …); `values` — поля под тип
/// (для `fill`: `"{percent}#{bar}#{crys}#{a100}#{a1000}#{amax}"`).
pub struct RichRow {
    label: String,
    kind: String,
    values: String,
    action: String,
    value: String,
}

impl RichRow {
    /// Полоса заполнения (заряд/респ): процент, подпись, тип кристалла + 3 кнопки.
    pub fn fill(label: impl Into<String>, values: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: "fill".into(),
            values: values.into(),
            action: String::new(),
            value: String::new(),
        }
    }
}

/// HORB-окно. Гарантирует корректный плоский wire-контракт `HORBConfig`.
#[derive(Default)]
pub struct Horb {
    title: String,
    text: String,
    buttons: Vec<Button>,
    list: Vec<ListRow>,
    rich_list: Vec<RichRow>,
    admin: bool,
}

impl Horb {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    /// Флаг доступа к admin-странице (шестерёнка у владельца здания).
    #[must_use]
    pub const fn admin(mut self, admin: bool) -> Self {
        self.admin = admin;
        self
    }

    #[must_use]
    pub fn button(mut self, b: Button) -> Self {
        self.buttons.push(b);
        self
    }

    /// Стандартная кнопка закрытия (`ВЫЙТИ` → `exit`).
    #[must_use]
    pub fn close_button(self) -> Self {
        self.button(Button::new("ВЫЙТИ", "exit"))
    }

    #[must_use]
    pub fn list_row(mut self, r: ListRow) -> Self {
        self.list.push(r);
        self
    }

    #[must_use]
    pub fn rich_row(mut self, r: RichRow) -> Self {
        self.rich_list.push(r);
        self
    }

    /// Сериализовать в плоский `HORBConfig`-JSON. Коллекции разворачиваются в
    /// `string[]`-кортежи ровно как ждёт клиент (чётность `buttons` гарантирована
    /// — каждая кнопка даёт ровно пару).
    fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::Map::new();
        obj.insert("title".into(), self.title.clone().into());
        obj.insert("text".into(), self.text.clone().into());
        obj.insert("back".into(), false.into());
        obj.insert("admin".into(), self.admin.into());
        if !self.buttons.is_empty() {
            let flat: Vec<String> = self
                .buttons
                .iter()
                .flat_map(|b| [b.label.clone(), b.action.clone()])
                .collect();
            obj.insert("buttons".into(), flat.into());
        }
        if !self.list.is_empty() {
            let flat: Vec<String> = self
                .list
                .iter()
                .flat_map(|r| [r.title.clone(), r.subtitle.clone(), r.action.clone()])
                .collect();
            obj.insert("list".into(), flat.into());
        }
        if !self.rich_list.is_empty() {
            let flat: Vec<String> = self
                .rich_list
                .iter()
                .flat_map(|r| {
                    [
                        r.label.clone(),
                        r.kind.clone(),
                        r.values.clone(),
                        r.action.clone(),
                        r.value.clone(),
                    ]
                })
                .collect();
            obj.insert("richList".into(), flat.into());
        }
        serde_json::Value::Object(obj)
    }

    /// Отправить окно игроку и записать `current_window = window_tag`
    /// (формат `"{kind}:{x}:{y}"` или `"{kind}"`, как ждут хендлеры кнопок).
    pub fn send(
        &self,
        state: &Arc<GameState>,
        tx: &mpsc::UnboundedSender<Vec<u8>>,
        pid: PlayerId,
        window_tag: impl Into<String>,
    ) {
        send_u_packet(tx, "GU", format!("horb:{}", self.to_json()).as_bytes());
        let tag = window_tag.into();
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = Some(tag);
            }
            Some(())
        });
    }
}
