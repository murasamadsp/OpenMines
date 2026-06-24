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

    /// Выпадающий список. `values` — клиентский формат `"0:label#1:label#"`,
    /// `key` — имя поля в `%R%`, `selected` — текущий индекс.
    pub fn dropdown(
        label: impl Into<String>,
        values: impl Into<String>,
        key: impl Into<String>,
        selected: i64,
    ) -> Self {
        Self {
            label: label.into(),
            kind: "drop".into(),
            values: values.into(),
            action: key.into(),
            value: selected.to_string(),
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

/// Элемент `canvas` в локальных координатах `(x, y)` от центра `canvasGUI`.
/// Это НИЗКИЙ уровень — обычно не трогается напрямую, его генерит `minimap()`.
/// Клиент (`ShowHORB`) аккумулирует ЦИФРЫ, потом применяет БУКВУ-команду
/// (`-18X` → x=-18), поэтому число идёт ПЕРЕД буквой, тип — последним перед `%`.
enum CanvasEl {
    /// Прямоугольник `=R`: позиция, размер, цвет `RRGGBB`.
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: String,
    },
    /// Кликабельная точка-ТП `=t` (маркер). `action` уходит серверу при клике
    /// (для телепорта — `"tp:x:y"`). Кодируется ДВУМЯ элементами массива.
    Tp { x: i32, y: i32, action: String },
}

impl CanvasEl {
    /// Развернуть в плоские строки `canvas` (Tp даёт пару: команда + action).
    fn push_to(&self, out: &mut Vec<String>) {
        match self {
            Self::Rect { x, y, w, h, color } => {
                out.push(format!("{x}X{y}Y{w}w{h}h=R%{color}"));
            }
            Self::Tp { x, y, action } => {
                out.push(format!("{x}X{y}Y=t"));
                out.push(action.clone());
            }
        }
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
    /// Элементы `canvas` (мини-карта). Заполняется `minimap()`.
    canvas: Vec<CanvasEl>,
    /// CSS сайзинга (`"canv-w=…;canv-h=…"`). Ставится `minimap()`.
    css: String,
    /// Карточка предмета/скилла/кристалла (`HORBConfig.card`), например `"i0:TP"`.
    card: String,
    /// Инвентарь/предметный грид (`HORBConfig.inv`), клиентский формат `id: top;!bottom`.
    inv: String,
    input_place: String,
    input_console: bool,
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

    #[must_use]
    pub fn input(mut self, placeholder: impl Into<String>, focus_console: bool) -> Self {
        self.input_place = placeholder.into();
        self.input_console = focus_console;
        self
    }

    #[must_use]
    pub fn card(mut self, card: impl Into<String>) -> Self {
        self.card = card.into();
        self
    }

    #[must_use]
    pub fn inventory(mut self, inv: impl Into<String>) -> Self {
        self.inv = inv.into();
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

    /// УМНАЯ мини-карта одним вызовом — БЕЗ ручной пиксельной математики.
    /// Разработчик задаёт намерение в МИРОВЫХ клетках, движок считает пиксели:
    /// - `center_x/y` — центр карты (клетки мира, обычно позиция здания);
    /// - `radius` — радиус в ЧАНКАХ (карта = `(2r+1)²` плиток);
    /// - `cell_empty(x,y)` — цвет клетки: `Some(true)`=пусто (зелёный),
    ///   `Some(false)`=порода (синий), `None`=вне мира (пропуск). 1:1 C# `ConvertMapPart`;
    /// - `markers` — кликабельные точки в МИРОВЫХ координатах `(x, y, action)`;
    ///   те, что попадают на карту, авто-позиционируются (`action` напр. `"tp:x:y"`).
    ///
    /// Сам ставит размер canvas (`css`) и маркер «вы здесь» в центре.
    #[must_use]
    pub fn minimap(
        mut self,
        center_x: i32,
        center_y: i32,
        radius: i32,
        cell_empty: impl Fn(i32, i32) -> Option<bool>,
        markers: &[(i32, i32, String)],
    ) -> Self {
        const PX: i32 = 18; // пикселей на чанк-плитку
        let (ccx, ccy) = (center_x.div_euclid(32), center_y.div_euclid(32));
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (cx, cy) = ((ccx + dx) * 32 + 16, (ccy + dy) * 32 + 16);
                let Some(empty) = cell_empty(cx, cy) else {
                    continue;
                };
                let color = if empty { "008000" } else { "6495ed" };
                // Y инвертируем: карта y-вниз, Unity y-вверх (север сверху).
                self.canvas.push(CanvasEl::Rect {
                    x: dx * PX,
                    y: -dy * PX,
                    w: PX,
                    h: PX,
                    color: color.into(),
                });
            }
        }
        // Маркер «вы здесь» (поверх плиток).
        self.canvas.push(CanvasEl::Rect {
            x: 0,
            y: 0,
            w: PX,
            h: PX,
            color: "ff3030".into(),
        });
        // Кликабельные точки в мировых координатах → авто-позиция на карте.
        for (mx, my, action) in markers {
            let (dcx, dcy) = (mx.div_euclid(32) - ccx, my.div_euclid(32) - ccy);
            if dcx.abs() <= radius && dcy.abs() <= radius {
                self.canvas.push(CanvasEl::Tp {
                    x: dcx * PX,
                    y: -dcy * PX,
                    action: action.clone(),
                });
            }
        }
        let side = (2 * radius + 1) * PX + PX;
        self.css = format!("canv-w={side};canv-h={side}");
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
        if !self.card.is_empty() {
            obj.insert("card".into(), self.card.clone().into());
        }
        if !self.inv.is_empty() {
            obj.insert("inv".into(), self.inv.clone().into());
        }
        if !self.input_place.is_empty() {
            obj.insert("input_place".into(), self.input_place.clone().into());
        }
        if self.input_console {
            obj.insert("input_console".into(), true.into());
        }
        if !self.canvas.is_empty() {
            let mut flat = Vec::new();
            for el in &self.canvas {
                el.push_to(&mut flat);
            }
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

    /// Отправить HORB без `PlayerUI.current_window`.
    /// Нужно для pre-auth окон, где игрок ещё не выбран.
    pub fn send_raw(&self, tx: &mpsc::UnboundedSender<Vec<u8>>) {
        self.emit(tx);
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
}

#[cfg(test)]
mod tests {
    use super::{Button, Horb, RichRow, Tab};

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
        assert!(json.get("card").is_none());
        assert!(json.get("inv").is_none());
        assert!(json.get("input_place").is_none());
        assert!(json.get("input_console").is_none());
    }

    #[test]
    fn card_and_inventory_are_emitted_unchanged() {
        let json = Horb::new("Auc")
            .card("i0:TP")
            .inventory("0: 2;!100$:1: ;!")
            .tab(Tab::active("Auc"))
            .button(Button::new("НАЗАД", "auc"))
            .to_json();

        assert_eq!(json["card"], "i0:TP");
        assert_eq!(json["inv"], "0: 2;!100$:1: ;!");
        assert_eq!(arr(&json, "tabs").len(), 2);
        assert_eq!(arr(&json, "buttons").len(), 4);
    }

    #[test]
    fn input_dialog_fields_are_emitted_only_when_requested() {
        let json = Horb::new("Input")
            .text("Введите")
            .input("Название...", true)
            .button(Button::new("OK", "ok:%I%"))
            .to_json();

        assert_eq!(json["input_place"], "Название...");
        assert_eq!(json["input_console"], true);
        assert_eq!(arr(&json, "buttons").len(), 4);
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
            .rich_row(RichRow::dropdown("Масштаб", "0:мелко#1:КРУПНО#", "isca", 1))
            .rich_row(RichRow::button("Прибыль: 999", "Забрать", "take_profit"))
            .to_json();
        let r = arr(&json, "richList");
        assert_eq!(r.len(), 25); // 5 строк × 5 полей
        // toggle row 1 (индексы 5..10): [label, "bool", "", key, "1"]
        assert_eq!(r[6], "bool");
        assert_eq!(r[8], "clan_lock");
        assert_eq!(r[9], "1");
        // uint row 2: value = default
        assert_eq!(r[10 + 1], "uint");
        assert_eq!(r[10 + 4], "50");
        // dropdown row 3: values = option list, action = key, value = selected.
        assert_eq!(r[15 + 1], "drop");
        assert_eq!(r[15 + 2], "0:мелко#1:КРУПНО#");
        assert_eq!(r[15 + 3], "isca");
        assert_eq!(r[15 + 4], "1");
        // button row 4: values = btn label, action = key
        assert_eq!(r[20 + 1], "button");
        assert_eq!(r[20 + 2], "Забрать");
        assert_eq!(r[20 + 3], "take_profit");
    }

    /// Canvas кодируется корректно через minimap.
    #[test]
    fn canvas_rect_encodes_for_client_parser() {
        let json = Horb::new("Тп")
            .minimap(0, 0, 1, |_x, _y| Some(true), &[(32, 32, "tp:32:32".into())])
            .to_json();
        let canvas = arr(&json, "canvas");
        assert_eq!(canvas.len(), 12);
        assert_eq!(canvas[0], "-18X18Y18w18h=R%008000"); // Empty/Green
        assert_eq!(canvas[9], "0X0Y18w18h=R%ff3030"); // Center marker
        assert_eq!(canvas[10], "18X-18Y=t");
        assert_eq!(canvas[11], "tp:32:32");
        assert_eq!(json["css"], "canv-w=72;canv-h=72"); // side = (2*1+1)*18+18 = 72
    }
}
