# 1. Зафиксировать режимы запуска

  Сервер должен поддерживать три режима:

  1. Dedicated server — текущий TCP/VPS.
  2. Local server — отдельный локальный процесс на 127.0.0.1.
  3. Embedded server — серверная библиотека внутри Unity-клиента.

  Во всех режимах игровая логика и legacy wire должны быть одинаковыми.

# 2. Сначала сделать local subprocess

  Это самый безопасный промежуточный этап:

- Unity запускает openmines-server как child process.
- Сервер получает временный data_dir, порт и admin token через CLI/env.
- Клиент подключается к 127.0.0.1.
- При выходе клиента сервер получает graceful shutdown.
- Логи и crash-код процесса передаются в Unity diagnostics.
- Мир хранится отдельно для каждого локального мира/профиля.

  Так можно проверить весь lifecycle без FFI и не трогать сетевой протокол.

# 3. Отделить transport от simulation

  В сервере нужно выделить абстракцию транспорта:

  Simulation
     ↓ GameEvent / typed effects
  Presentation
     ↓
  Transport adapter
     ├── TCP sockets
     ├── local IPC
     └── in-process channels

  Simulation не должна знать, TCP это, Unity IPC или embedded channel. Legacy packet encoding остаётся на presentation/transport boundary.

# 4. Сделать стабильный server library API

  Вместо запуска только через main.rs выделить crate/API примерно такого уровня:

  pub struct EmbeddedServerConfig { ... }

  pub struct EmbeddedServer {
      // runtime, world, persistence, sessions
  }

  impl EmbeddedServer {
      pub fn start(config: EmbeddedServerConfig) -> Result<Self>;
      pub fn connect(&self, client: ClientEndpoint) -> Result<SessionHandle>;
      pub fn shutdown(self) -> Result<()>;
  }

  Важно:

- не экспортировать внутренний ECS;
- не отдавать наружу GameState;
- не позволять Unity мутировать сервер напрямую;
- наружу только connect/send/receive/shutdown/diagnostics.

# 5. Выбрать IPC-модель

  Есть три варианта.

#### Вариант A — loopback TCP

  Простейший и почти бесплатный:

  Unity → 127.0.0.1:port → Rust server

  Плюсы: минимум изменений, идеальная проверка wire-паритета.
  Минусы: отдельный процесс, сложнее packaging.

#### Вариант B — native FFI

  Rust экспортирует C ABI:

  openmines_server_create
  openmines_server_send
  openmines_server_poll
  openmines_server_destroy

  Unity вызывает это через DllImport.

  Плюсы: один процесс.
  Минусы: сложные lifetime/threading/crash boundaries, native libraries для каждой платформы.

#### Вариант C — Unity Native Plugin + shared memory/channels

  Быстрее TCP, но сложнее всего в отладке и packaging. Я бы не начинал с него.

  Практический порядок: A → B. Вариант C только если появится измеренная необходимость.

### 6. Вынести runtime lifecycle

  Embedded-сервер не должен сам владеть Unity main thread. Нужны:

- отдельный Rust runtime thread;
- отдельный simulation thread;
- bounded inbound/outbound queues;
- poll() или callback для исходящих пакетов;
- гарантированный shutdown без блокировки Unity;
- обработка Unity pause/background;
- watchdog и диагностика deadlock/crash.

  Например:

  Unity main thread
     ↓ send queue
  Rust server thread
     ↓ outbound queue
  Unity Update()

  Никаких callback-ов из Rust прямо в Unity main thread без явного dispatcher-а.

### 7. Сделать embedded transport совместимым с legacy client

  Поскольку клиент legacy, есть два пути:

  1. Оставить существующий клиентский TCP-код и использовать local subprocess.
  2. Для настоящего embedded режима добавить в клиенте тонкий transport adapter, который выглядит для packet parser так же, как socket.

  Менять packet parsing нельзя. Допустимо заменить только источник/доставку байтов:

  Legacy packet parser
          ↑
    transport facade
     ├── TCP socket
     └── embedded native queue

  Wire bytes, порядок пакетов и названия событий остаются неизменными.

### 8. Разделить локальные и серверные данные

  Для embedded режима заранее определить:

- путь локального мира;
- SQLite database;
- миграции;
- lock-файлы;
- резервные копии;
- версию формата мира;
- reset/regen;
- экспорт мира на dedicated server;
- импорт мира обратно.

  Нельзя использовать один и тот же data/ одновременно несколькими embedded-инстансами.

### 9. Собрать native binaries для платформ

  Потребуются артефакты:

  Windows: openmines_server.dll
  macOS: libopenmines_server.dylib
  Linux: libopenmines_server.so
  Android/iOS: отдельные native builds

  Для Unity:

- .bundle/.dylib на macOS;
- .dll на Windows;
- .so на Linux/Android;
- архитектуры x86_64, arm64;
- IL2CPP compatibility;
- code signing и notarization на macOS;
- лицензирование SQLite/прочих native dependencies.

### 10. Тестировать одинаковость режимов

  Нужен один общий protocol/conformance suite:

  same client command
     ↓
  TCP server result
  embedded server result
     ↓
  identical packet sequence and payload

  Проверять:

- connect/auth;
- Player.Init order;
- movement/dig/build;
- chat;
- programmer;
- reconnect;
- shutdown;
- persistence;
- malformed packets;
- timing-sensitive actions.

  Для каждого golden-теста сравнивать не только события, но и байты пакетов.

### 11. Добавить режим offline/local gameplay

  После технического embedded режима можно решать продуктовые вопросы:

- локальная игра без аккаунта;
- локальный admin;
- single-player world;
- bots/NPC;
- cloud sync;
- LAN hosting;
- переход local → dedicated;
- защита локального мира от несовместимых версий.

  Это уже не должно попадать в server kernel без отдельного требования.

### Реалистичный порядок

  Текущая ECS/typed migration
          ↓
  Local subprocess на loopback TCP
          ↓
  Transport facade в клиенте
          ↓
  Server library API
          ↓
  Native FFI plugin
          ↓
  Embedded queues + Unity lifecycle
          ↓
  Cross-platform packaging
          ↓
  Conformance/performance/persistence hardening
          ↓
  Offline/LAN/cloud product features

  Главное архитектурное правило: сначала добиться режима local subprocess с абсолютно тем же wire, затем переносить процесс внутрь клиента. Прямой прыжок сразу в FFI даст много
  инфраструктурной работы и будет мешать текущей миграции сервера.
