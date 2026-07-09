# Паритет генерации мира: побайтовая сверка с C #

Дата: 2026-06-23.

## Что сверяли и итог

И **скелет**, и **заливка секторов** теперь генерируются в Rust **побайтово
идентично** эталонному C# (`Sectors` + `SectorFiller` + `Sector`) — **0 расхождений**:

- **Скелет** (RidgedMulti-шум → `AddW`×3 → `End`): 12 сидов (включая `0`,
  `i32::MIN/MAX`, отрицательные) × размеры 64²…256².
- **Заливка** (детерминированный C#-харнесс, засев по схеме Rust): 30+ сидов
  (1-значные…7-значные) × размеры 64²…224² — везде байт-в-байт.

Найдено и исправлено **6 реальных багов** паритета (см. ниже). Остатка нет:
ранее наблюдавшиеся ~0.02–0.17% одиночных клеток оказались **не** «неустранимым
libm», а конкретным багом — int-overflow в `simplex_noise` (баг 6).

## Три независимых `Random` в C# и как их засеяли

C# использует три источника случайности:

| Источник | Конструктор | Засев в харнессе |
| - | - | - |
| `Sectors.r` (скелет) | сид-перегрузка `Sectors(int seed,(w,h))` | `seed` напрямую |
| `SectorFiller.rand` (заливка) | `new Random()` без сида (стр. 13) | инжект общего Random |
| `Sector.r` (`GenerateInsides`) | `static new Random()` без сида (стр. 9) | тот же общий Random |
| шум `NotTypedNoise` | `Seed = DateTime.Ticks` (не задан) | `Seed = rand.Next()` |

Боевой C# недетерминирован (unseeded → каждый запуск иной мир). Rust-порт даёт
детерминизм: фикс-`seed` + **единый** per-sector `Random((seed+seq)*0x1337)`,
из которого тянутся и `GenerateInsides`, и заливка (C# два потока слиты в один).
Харнесс воспроизводит ровно эту схему засева, поэтому заливка стала сверяема.

## Харнесс (standalone dotnet, воспроизведение)

Цель: прогнать **дословный** C# `Sectors` через сид-конструктор и выгрузить
`map[x*h+y].value` (0/1/2), затем сравнить с Rust `generate_skeleton`.

```
mkdir -p /tmp/genparity/src && cd /tmp/genparity
dotnet new console
# дословные исходники:
cp crates/openmines-shared/src/world/anl_reference/*.cs            src/   # шум TinkerWorX (9 файлов)
cp docs/reference/server_reference/.../Generator/SectorCell.cs src/
cp docs/reference/server_reference/.../Generator/Sectors.cs    src/
```

Правки в копиях харнесса (только чтобы скомпилировать скелет без боевых зависимостей):

- `Sectors.cs`: `RcherNZ.AccidentalNoise` → `TinkerWorX.AccidentalNoiseLibrary`;
  тело `DetectAndFillSectors()` выпотрошено (недетерминированная заливка, не тестируется).
- `ImplicitModuleBase.cs`: удалён implicit-оператор `ImplicitConstant` (тип не входит
  в anl_reference-подмножество, на пути скелета не используется).
- Стабы `CellType` (Empty=32/BlackRock=114/RedRock=117) и `World` (no-op).

`Program.cs`: `new Sectors(seed,(w,h))` → `GenerateENoise(15,1,Cubic)` +
`AddW(15,1,Linear)` + `AddW(25,5,Linear)` + `AddW(35,20,Quintic)` + `End()` →
дамп `map[x*h+y].value` как байты 0/1/2.

Rust-сторона: `generate_skeleton(w,h,seed)` (маппинг `BLACK_ROCK→2, RED_ROCK→1,
_→0`). `cmp` двух бинарей.

## Найденные баги и фиксы

**Скелет (1 баг):** `sectors_gen.rs` — `Sectors.min/mid/max` были **`f32`**, в C#
это **`double`** (`public double min, mid, max`). Нормализация в C# идёт
`(float)((value-min)/(max-min))` в double, а `mid` копится в double по 32k+
клеткам. f32-накопление сдвигало порог `mid+0.45f`: пограничные клетки меняли
класс, и `CleanCs`/`Boom` + общий `r` усиливали это до ~16 % байт на краевых
сидах. **Фикс:** `min/mid/max` → `f64`, нормализация и порог в f64 с `f64::from`,
усечение в f32 — дословно C#.

**Заливка (5 багов) в `generator.rs`/`sector_palette.rs`/`anl.rs`:**

1. **`random_sized_parts` cap** — Rust force-push'ил перекрывающийся сегмент после
   `guard≥1000`, C# крутит `while`-overlap безлимитно до зазора. **Фикс:** убран
   cap, дословный C#-`while`.
2. **Накопление мелких секторов** — C# сбрасывает аккумулятор `ce` ТОЛЬКО после
   сектора ≥50; компоненты <50 **копятся** в один «сектор». Rust их выбрасывал →
   находил 6 секторов вместо 18. **Фикс:** `acc` копит компоненты до порога 50,
   габариты — от последнего компонента.
3. **Сброс типов между attempt'ами** — C# `c.type` **сохраняется** между проходами
   (`c.type = inrange ? key : c.type`), Rust обнулял `types` в EMPTY каждый attempt
   → клетки, не попавшие в part, терялись. **Фикс:** `types` инициализируется
   EMPTY один раз, не сбрасывается. *(Это дало 48 %→0.02 % расхождений.)*
4. **Палитра tier 1 non-gig** — Rust `crys` = `[Green,Blue,XBlue]`, C# `[Green,
   Blue]` (лишний X_BLUE менял `lencry` → десинк RNG). **Фикс:** убран X_BLUE.

5. **int-overflow в `simplex_noise`** (`anl.rs`) — C# `(i + j) * G2` складывает
   `i,j` как **int**, и сумма ПЕРЕПОЛНЯЕТ i32 при больших координатах (высокие
   octaves×freq×lac → inner-coord ~1.5e9, `i+j` > i32::MAX → two's-complement
   wrap), и лишь потом `* G2`. Rust складывал в f64 (без переполнения) → расход
   только у базиса Simplex на глубоких октавах, проявлялся как горстка одиночных
   клеток. **Фикс:** `f64::from(i.wrapping_add(j)) * G2`. *(Это убрало последние
   ~0.02–0.17%; до фикса ошибочно списано на «неустранимый libm».)*

Метод поиска: дамп `fr.Get` для всех `тип×базис×interp` (+свип octaves/freq/lac)
выявил, что расходится ровно `basis=Simplex` при высоких octaves → локализация в
`(i+j)` int-арифметике.

Что **не** было причиной (проверено бит-в-бит): весь `DotnetRng` (`Next`/
`NextDouble`), `Math.Cos/Sin/Pow/Log/Exp`, `fr.Get` всех прочих базисов,
`generate_insides` args, `parts`. Базовые трансценденты .NET и Rust на этой
платформе совпадают побайтово — «libm-гипотеза» опровергнута эмпирически.

## Регресс-защита (golden FNV-1a64)

- `sectors_gen::…::skeleton_matches_csharp_reference_golden` — скелет seed=42
  128×256 = `0x56174d1667aa7649`.
- `generator::…::full_world_fill_matches_csharp_reference_golden` — полный мир
  (скелет+заливка) seed=7 64×64 = `0x791440a4206be5c4` (байт-точно совпал с C#).

Матрица сверки скелета: сиды `-2147483648, -42, -1, 0, 7, 42, 1337, 12345,
999983, 88888888, 2147483646, 2147483647` × размеры `64²…256²` — все IDENTICAL.
