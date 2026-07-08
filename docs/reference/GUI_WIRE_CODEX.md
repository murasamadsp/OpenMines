# GUI/Wire Codex

Цель: единый реестр `клиентское действие -> TY event/button -> серверный
handler -> server packets`. Перед правками GUI/wire сначала обновлять этот файл,
потом код и smoke.

Формат строк стабильный и grep-friendly:

```text
route | client | ty | payload | server | packets | notes
```

## Auth

```text
auth.login.open | AU invalid/noauth | AU | <uniq>_NO_<...> | auth/login.rs::send_auth_failure | cf,BI,HB,GU | открывает GUI авторизации
auth.register.menu | GUI auth window | GUI_ | {"b":"newakk"} | auth/gui_flow.rs::handle_gui_auth_button | GU | форма регистрации
auth.register.nick | GUI auth window | GUI_ | {"b":"newnick:<nick>"} | auth/gui_flow.rs::handle_gui_auth_button | GU | форма пароля
auth.register.password | GUI auth window | GUI_ | {"b":"passwd:<password>"} | auth/gui_flow.rs::finalize_auth | AH,cf,Gu,<Player.Init> | AH нужен клиенту для сохранения логина
auth.reconnect | saved credentials | AU | <uniq>_<id>_<md5(hash+sid)> | auth/login.rs::handle_auth | cf,Gu,<Player.Init> | AH не шлётся
```

## Programmator

Источник деталей: `docs/reference/PROGRAMMATOR_GUI_PROTOCOL.md`.

```text
prog.menu.open | GUIManager.OnProgButton | Pope | ignored | social/buildings.rs::handle_programmator_pope_menu | GU | список программ, не запуск
prog.create.dialog | HORB button | GUI_ | {"b":"createprog"} | ui/gui_buttons.rs::open_create_prog_dialog | GU | поле имени программы
prog.create.confirm | HORB input button | GUI_ | {"b":"createprog:<name>"} | ui/gui_buttons.rs::handle_create_prog | Gu,#P,Gu | выбирает created program, открывает редактор
prog.open | HORB list button | GUI_ | {"b":"openprog:<id>"} | ui/gui_buttons.rs::handle_open_prog | Gu,#P,Gu | только owned program
prog.save.start | ProgrammerView.SendAndStartProgram | PROG | [len:i32][id:i32][compiled][source] | social/misc.rs::handle_prog_ty | Gu,optional @T,@P,BH,#p,optional OK | успешный старт должен слать #p последним, иначе @P 1 снова показывает editor object
prog.stop | GUIManager/ProgPanel stop | pRST | empty | social/misc.rs::handle_prog_ty | Gu,@P,BH | только если реально был running
prog.preopen.reset | GUIManager.OnProgButton pre-open | pRST | empty | social/misc.rs::handle_prog_ty | none or @P0 only on missing state | stopped selected не должен открывать #P
prog.delete | ProgrammerView delete | PDEL | <id> | social/misc.rs::handle_prog_ty | none | C# parity: wire-silent
prog.rename.open | ProgrammerView rename | PREN | <id> | social/misc.rs::handle_prog_ty | GU | HORB input dialog
prog.rename.confirm | HORB input button | GUI_ | {"b":"rename:<id>:<name>"} | ui/gui_buttons.rs::handle_rename_prog | #p,Gu | update, не #P
prog.copy | ProgrammerView copy | PCOP | <id> | social/misc.rs::handle_prog_ty | GU | refresh program list
prog.login.selected | Player.Init | server push | selected_program from DB | player/init.rs::init_player | @P,BH,#p in init stream | не открывать #P поверх игры; для running-программы #p идёт после @P/BH и скрывает editor view
```

## Gameplay Toggles

```text
toggle.autodig | client toggle | TADG | empty | social/misc.rs::handle_auto_dig_toggle | BD | dirty player settings
toggle.aggression | client toggle | TAGR | empty | social/misc.rs::handle_aggression_toggle | BA | dirty player settings
```

## Common HORB

```text
settings.open | settings button | Sett | optional rich payload | social/misc.rs::handle_sett_ty | GU or #S | open/save settings
buildings.mine | buildings button | Blds | empty | social/buildings.rs::handle_my_buildings_list | GU | own buildings list
buildings.place.menu | HORB button | GUI_ | {"b":"open_buildings"} | ui/gui_buttons.rs::handle_buildings_menu | GU | placement menu
buildings.place | HORB button | GUI_ | {"b":"bld_place:<code>"} | social/buildings.rs::handle_place_building | P$,OK/Gu/HB side effects | validates cost/area/config
admin.open.pack | admin gear on pack window | ADMN | empty | dispatch/ty.rs ADMN branch | GU | current_window=`pack:x:y`, opens generic pack admin
admin.open.resp | admin gear on resp window | ADMN | empty | dispatch/ty.rs ADMN branch | GU | current_window=`resp:x:y`, opens resp admin
admin.open.market | admin gear on market window | ADMN | empty | dispatch/ty.rs ADMN branch | GU | current_window=`market:x:y`, opens market admin
admin.open.up | admin gear on up window | ADMN | empty | dispatch/ty.rs ADMN branch | GU | current_window=`up:x:y`, opens up admin
```

## Smoke Coverage

`scripts/dev-smoke.sh` сейчас проверяет:

- `auth.login.open`;
- `auth.register.*`;
- `auth.reconnect`;
- `prog.menu.open`;
- `prog.create.confirm`;
- `prog.open`;
- `prog.rename.*`;
- `prog.copy`;
- `prog.delete`;
- `prog.save.start`;
- `prog.stop`;
- `prog.login.selected`;
- `toggle.aggression`;
- `settings.open/save`;
- `buildings.mine`;
- `buildings.place.menu`;
- `buildings.place` (Spot placement);
- `admin.open.pack`;
- `Xdig`, `Xmov`, `PO/PI` liveness.

Следующие кандидаты для smoke: auction/market, resp admin, up admin, clan routes.
