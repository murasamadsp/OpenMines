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
//! - `tabs` — пары `[label, action, …]`; активная вкладка имеет пустой action.
//! - `crys_lines` — крис-секция market/box (ровно 6 строк).
//!
//! Раньше каждый хендлер лепил JSON руками (gun GUI вообще слал `tabs:[{объект}]` —
//! а `HORBConfig.tabs` это `string[]`, `JsonUtility` это не парсит → окно не
//! открывалось). Этот модуль централизует сборку и устраняет такие расхождения.

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

/// Вкладка окна `tabs` (плоская пара `[label, action]`). Активная (текущая)
/// вкладка имеет ПУСТОЙ action — клиент рисует её неактивной без листенера
/// (`PopupManager.ShowHORB`: `tabs[n+1]==""` → `openedTabPrefab`); остальные
/// кликабельны и при клике шлют свой action.
pub struct Tab {
    label: String,
    action: String,
}

impl Tab {
    /// Кликабельная вкладка: подпись + action перехода.
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
        }
    }

    /// Текущая (активная) вкладка — без action (не кликабельна).
    pub fn active(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: String::new(),
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

    /// Строка-текст (только подпись, без контролов).
    pub fn text(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: "text".into(),
            values: String::new(),
            action: String::new(),
            value: String::new(),
        }
    }

    /// Чекбокс. `key` — имя поля (уходит в `%R%`-сабмит как `key:0|1`),
    /// `on` — начальное состояние.
    pub fn toggle(label: impl Into<String>, key: impl Into<String>, on: bool) -> Self {
        Self {
            label: label.into(),
            kind: "bool".into(),
            values: String::new(),
            action: key.into(),
            value: if on { "1".into() } else { "0".into() },
        }
    }

    /// Числовой ввод. `key` — имя поля, `default` — стартовое значение.
    pub fn uint(label: impl Into<String>, key: impl Into<String>, default: i64) -> Self {
        Self {
            label: label.into(),
            kind: "uint".into(),
            values: String::new(),
            action: key.into(),
            value: default.to_string(),
        }
    }

    /// Строка с кнопкой действия. `btn_label` — текст кнопки (пустой → кнопка
    /// скрыта), `action` — что уходит серверу при клике.
    pub fn button(
        label: impl Into<String>,
        btn_label: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            kind: "button".into(),
            values: btn_label.into(),
            action: action.into(),
            value: String::new(),
        }
    }
}

/// Прямоугольник `canvas` (мини-карта). Клиент (`ShowHORB`, тип `=R`) рисует
/// его в `canvasGUI` по локальным координатам `(X, Y)` от центра, размером `w×h`,
/// цветом `color` (RRGGBB hex, альфа по умолчанию 255). `X`/`Y` (заглавные) —
/// per-element абсолютные (сбрасываются), в отличие от накапливающегося `x`/`y`.
pub struct CanvasRect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: String,
}

impl CanvasRect {
    pub fn new(x: i32, y: i32, w: i32, h: i32, color: impl Into<String>) -> Self {
        Self {
            x,
            y,
            w,
            h,
            color: color.into(),
        }
    }

    /// Кодировка элемента canvas: `"X{x}Y{y}w{w}h{h}=R%{color}"`.
    fn encode(&self) -> String {
        format!(
            "X{}Y{}w{}h{}=R%{}",
            self.x, self.y, self.w, self.h, self.color
        )
    }
}

/// HORB-окно. Гарантирует корректный плоский wire-контракт `HORBConfig`.
#[derive(Default)]
pub struct Horb {
    title: String,
    text: String,
    buttons: Vec<Button>,
    tabs: Vec<Tab>,
    list: Vec<ListRow>,
    rich_list: Vec<RichRow>,
    admin: bool,
    /// Крис-секция (market/box): ровно 6 строк `"left:right:den:value:label"`.
    crys_lines: Vec<String>,
    crys_left: String,
    crys_right: String,
    /// Режим покупки (`crys_buy`): слайдеры считают цену покупки, не продажи.
    crys_buy: bool,
    /// Прямоугольники `canvas` (мини-карта телепорта).
    canvas_rects: Vec<CanvasRect>,
    /// CSS сайзинга (`"canv-w=…;canv-h=…"` и т.п.).
    css: String,
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

    /// Добавить вкладку. Активная — через `Tab::active`, остальные `Tab::new`.
    #[must_use]
    pub fn tab(mut self, t: Tab) -> Self {
        self.tabs.push(t);
        self
    }

    /// Крис-секция: подписи слева/справа, режим покупки и ровно 6 строк
    /// `"left:right:den:value:label"` (клиент требует `crys_lines.Length==6`).
    #[must_use]
    pub fn crystals(
        mut self,
        left: impl Into<String>,
        right: impl Into<String>,
        buy: bool,
        lines: Vec<String>,
    ) -> Self {
        self.crys_left = left.into();
        self.crys_right = right.into();
        self.crys_buy = buy;
        self.crys_lines = lines;
        self
    }

    /// Добавить прямоугольник на canvas (мини-карта).
    #[must_use]
    pub fn rect(mut self, r: CanvasRect) -> Self {
        self.canvas_rects.push(r);
        self
    }

    /// CSS сайзинга canvas (`"canv-w=360;canv-h=360"`).
    #[must_use]
    pub fn css(mut self, css: impl Into<String>) -> Self {
        self.css = css.into();
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
        if !self.css.is_empty() {
            obj.insert("css".into(), self.css.clone().into());
        }
        if !self.canvas_rects.is_empty() {
            let flat: Vec<String> = self.canvas_rects.iter().map(CanvasRect::encode).collect();
            obj.insert("canvas".into(), flat.into());
        }
        if !self.buttons.is_empty() {
            let mut flat: Vec<String> = self
                .buttons
                .iter()
                .flat_map(|b| [b.label.clone(), b.action.clone()])
                .collect();
            // Escape (клиент `PopupManager.cs:1827/1835`) кликает ПОСЛЕДНЮЮ кнопку.
            // Гарантируем, что это `exit`, чтобы Escape всегда закрывал окно
            // (иначе Escape жмёт «назад»/«ок» и окно не закрыть быстро).
            if flat.last().map(String::as_str) != Some("exit") {
                flat.push("ВЫЙТИ".into());
                flat.push("exit".into());
            }
            obj.insert("buttons".into(), flat.into());
        }
        if !self.tabs.is_empty() {
            let flat: Vec<String> = self
                .tabs
                .iter()
                .flat_map(|t| [t.label.clone(), t.action.clone()])
                .collect();
            obj.insert("tabs".into(), flat.into());
        }
        if !self.crys_lines.is_empty() {
            obj.insert("crys_left".into(), self.crys_left.clone().into());
            obj.insert("crys_right".into(), self.crys_right.clone().into());
            obj.insert("crys_lines".into(), self.crys_lines.clone().into());
            if self.crys_buy {
                obj.insert("crys_buy".into(), true.into());
            }
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

    /// Сериализовать в GU-payload (`"horb:{json}"`) и отправить.
    fn emit(&self, tx: &mpsc::UnboundedSender<Vec<u8>>) {
        send_u_packet(tx, "GU", format!("horb:{}", self.to_json()).as_bytes());
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
        self.emit(tx);
        let tag = window_tag.into();
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = Some(tag);
            }
            Some(())
        });
    }

    /// Отправить окно БЕЗ записи `current_window`. Для окон, чьи кнопки
    /// self-contained и не резолвят координаты из `current_window` (clan-меню) —
    /// сохраняет прежнее поведение (clan-окна его не трекали).
    pub fn send_untracked(&self, tx: &mpsc::UnboundedSender<Vec<u8>>) {
        self.emit(tx);
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, CanvasRect, Horb, RichRow, Tab};

    fn arr<'a>(v: &'a serde_json::Value, key: &str) -> &'a Vec<serde_json::Value> {
        v.get(key)
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("поле `{key}` отсутствует или не массив"))
    }

    /// `buttons`/`tabs` ОБЯЗАНЫ быть чётными (клиент: action по `[2n+1]`),
    /// `crys_lines` ровно 6 (клиент требует `Length==6`). Builder это гарантирует.
    #[test]
    fn flat_arrays_have_client_required_cardinality() {
        let lines: Vec<String> = (0..6).map(|i| format!("0:0:{i}:0:lbl")).collect();
        let json = Horb::new("Market")
            .tab(Tab::active("ПРОДАЖА"))
            .tab(Tab::new("Покупка", "buycrys"))
            .crystals(" ", "цена", true, lines)
            .button(Button::new("sell", "sell:%M%"))
            .close_button()
            .to_json();

        let buttons = arr(&json, "buttons");
        assert_eq!(
            buttons.len() % 2,
            0,
            "buttons не чётный → NRE PopupManager:222"
        );
        assert_eq!(buttons.len(), 4); // sell + ВЫЙТИ = 2 пары

        let tabs = arr(&json, "tabs");
        assert_eq!(tabs.len() % 2, 0, "tabs не чётный");
        assert_eq!(tabs[1], ""); // активная вкладка → пустой action
        assert_eq!(tabs[3], "buycrys");

        assert_eq!(arr(&json, "crys_lines").len(), 6);
        assert_eq!(json["crys_buy"], true);
        assert_eq!(json["crys_left"], " ");
    }

    /// Пустые коллекции не сериализуются (клиент трактует отсутствие как null).
    #[test]
    fn empty_collections_are_omitted() {
        let json = Horb::new("X").to_json();
        assert!(json.get("tabs").is_none());
        assert!(json.get("crys_lines").is_none());
        assert!(json.get("crys_buy").is_none());
        assert!(json.get("buttons").is_none());
        assert!(json.get("canvas").is_none());
        assert!(json.get("css").is_none());
    }

    /// Escape кликает ПОСЛЕДНЮЮ кнопку (клиент `PopupManager.cs:1835`). Builder
    /// гарантирует, что последняя кнопка — `exit`, иначе авто-добавляет её
    /// (но не дублирует, если `close_button` уже её поставил).
    #[test]
    fn exit_is_always_the_last_button() {
        // Окно без явного close → exit авто-добавлен последним.
        let json = Horb::new("X")
            .button(Button::new("Назад", "back"))
            .to_json();
        let b = arr(&json, "buttons");
        assert_eq!(b[b.len() - 1], "exit");
        assert_eq!(b.len(), 4); // [Назад, back, ВЫЙТИ, exit]

        // close_button уже даёт exit последним → не дублируется.
        let json2 = Horb::new("X")
            .button(Button::new("Назад", "back"))
            .close_button()
            .to_json();
        let b2 = arr(&json2, "buttons");
        assert_eq!(b2.len(), 4); // дубля exit нет
        assert_eq!(b2[b2.len() - 1], "exit");
    }

    /// richList-формы кодируются 5-кортежами `[label, kind, values, key, value]`
    /// ровно как ждёт клиент `ShowHORB` (toggle=bool, uint, drop, button, text).
    #[test]
    fn rich_list_form_kinds_encode_as_five_tuples() {
        let json = Horb::new("Админка")
            .rich_row(RichRow::text("Прочность: 1000/1000"))
            .rich_row(RichRow::toggle("Закланить", "clan_lock", true))
            .rich_row(RichRow::uint("Стоимость", "cost", 50))
            .rich_row(RichRow::button("Прибыль: 999", "Забрать", "take_profit"))
            .to_json();
        let r = arr(&json, "richList");
        assert_eq!(r.len(), 20); // 4 строки × 5 полей
        // toggle row 1 (индексы 5..10): [label, "bool", "", key, "1"]
        assert_eq!(r[6], "bool");
        assert_eq!(r[8], "clan_lock");
        assert_eq!(r[9], "1");
        // uint row 2: value = default
        assert_eq!(r[10 + 1], "uint");
        assert_eq!(r[10 + 4], "50");
        // button row 3: values = btn label, action = key
        assert_eq!(r[15 + 1], "button");
        assert_eq!(r[15 + 2], "Забрать");
        assert_eq!(r[15 + 3], "take_profit");
    }

    /// Canvas-rect кодируется как `"X{x}Y{y}w{w}h{h}=R%{color}"` (клиент `ShowHORB`
    /// тип `=R`). `=R` ОБЯЗАН быть последним перед `%` (иначе клиент не распознает).
    #[test]
    fn canvas_rect_encodes_for_client_parser() {
        let json = Horb::new("Тп")
            .css("canv-w=360;canv-h=360")
            .rect(CanvasRect::new(-18, 36, 18, 18, "6495ed"))
            .to_json();
        let canvas = arr(&json, "canvas");
        assert_eq!(canvas.len(), 1);
        assert_eq!(canvas[0], "X-18Y36w18h18=R%6495ed");
        assert_eq!(json["css"], "canv-w=360;canv-h=360");
    }
}
