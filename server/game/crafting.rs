//! Рецепты и логика крафтера.
//!
//! Рецепты захардкожены для простоты (в референсе — JSON-файлы в `recipies/`).
//! Стоимость хранится в кристаллах (id 0..5) и предметах инвентаря (id 0..46).

/// Стоимость ингредиента: `(id, count)`.
#[derive(Debug, Clone, Copy)]
pub struct Cost {
    pub id: i32,
    pub num: i32,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: i32,
    /// Что производится: item id в инвентаре, `num` — штук за один запуск.
    pub result: Cost,
    pub cost_crys: &'static [Cost],
    pub cost_res: &'static [Cost],
    /// Секунд на одну единицу результата.
    pub time_sec: i32,
    pub title: &'static str,
}

/// Глобальный список рецептов. Первый рецепт — демо «первый шпак».
pub fn recipes() -> &'static [Recipe] {
    RECIPES
}

pub fn recipe_by_id(id: i32) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.id == id)
}

static RECIPES: &[Recipe] = &[
    Recipe {
        id: 0,
        result: Cost { id: 0, num: 1 },
        cost_crys: &[Cost { id: 0, num: 50 }],
        cost_res: &[],
        time_sec: 5,
        title: "Тп-шпак",
    },
    Recipe {
        id: 1,
        result: Cost { id: 1, num: 1 },
        cost_crys: &[Cost { id: 1, num: 100 }],
        cost_res: &[],
        time_sec: 10,
        title: "Респ-шпак",
    },
    Recipe {
        id: 2,
        result: Cost { id: 5, num: 1 },
        cost_crys: &[Cost { id: 1, num: 50 }, Cost { id: 2, num: 20 }],
        cost_res: &[],
        time_sec: 10,
        title: "Бомба",
    },
    Recipe {
        id: 3,
        result: Cost { id: 6, num: 1 },
        cost_crys: &[Cost { id: 3, num: 30 }],
        cost_res: &[],
        time_sec: 15,
        title: "Защита",
    },
    Recipe {
        id: 4,
        result: Cost { id: 7, num: 1 },
        cost_crys: &[Cost { id: 2, num: 100 }, Cost { id: 4, num: 20 }],
        cost_res: &[],
        time_sec: 30,
        title: "Разрушитель",
    },
    Recipe {
        id: 5,
        result: Cost { id: 35, num: 10 },
        cost_crys: &[Cost { id: 0, num: 20 }],
        cost_res: &[],
        time_sec: 5,
        title: "Полимер",
    },
    Recipe {
        id: 6,
        result: Cost { id: 40, num: 1 },
        cost_crys: &[Cost { id: 5, num: 50 }],
        cost_res: &[],
        time_sec: 20,
        title: "C190",
    },
    Recipe {
        id: 7,
        result: Cost { id: 29, num: 1 },
        cost_crys: &[Cost { id: 0, num: 200 }],
        cost_res: &[],
        time_sec: 60,
        title: "Склад-шпак",
    },
];
