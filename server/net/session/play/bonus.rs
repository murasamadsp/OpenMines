//! Ежедневный бонус (кнопка БОНУСЫ в клиенте → TY-событие `GDon`).
//!
//! НАМЕРЕННАЯ ДЕВИАЦИЯ от C# референса: в C# `GDonPacket` — заглушка (декод без
//! логики; донат там = реальные деньги, не реализован). По явному требованию
//! пользователя кнопка превращена в ежедневный бонус: клик → начисление раз в
//! кулдаун. Параметры (7 часов / 1 000 000 $) заданы пользователем.

use crate::game::player::{PlayerFlags, PlayerStats};
use crate::net::auction::now_unix;
use crate::net::session::prelude::*;

/// Кулдаун между клеймами бонуса (7 часов, в секундах).
const BONUS_COOLDOWN_SECS: i64 = 7 * 3600;
/// Размер бонуса (деньги).
const BONUS_REWARD: i64 = 1_000_000;

/// Доступен ли бонус: прошло ≥ кулдауна с последнего клейма (0 = ни разу → да).
#[must_use]
pub fn bonus_available(last_bonus_at: i64) -> bool {
    now_unix() - last_bonus_at >= BONUS_COOLDOWN_SECS
}

/// Исход попытки клейма бонуса.
enum ClaimOutcome {
    /// Начислено; новые `money`/`creds` для пакета `P$`.
    Claimed { money: i64, creds: i64 },
    /// Кулдаун ещё не вышел; осталось секунд.
    NotReady { remaining: i64 },
}

/// Обработка `GDon` (клик по кнопке БОНУСЫ). Sub-payload (`METHOD`) игнорируется.
pub fn handle_bonus_claim(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let now = now_unix();
    // Атомарно под ECS-локом: проверка кулдауна + начисление в одном замыкании,
    // чтобы спам `GDon` не дал двойной клейм. Начисляем в ECS (онлайн-игрок),
    // не прямым DB-write — иначе разойдётся с кэшем и flush затрёт.
    let outcome = state.modify_player(pid, |ecs, entity| {
        let last = ecs.get::<PlayerStats>(entity)?.last_bonus_at;
        if now - last < BONUS_COOLDOWN_SECS {
            return Some(ClaimOutcome::NotReady {
                remaining: BONUS_COOLDOWN_SECS - (now - last),
            });
        }
        let (money, creds) = {
            let mut pstats = ecs.get_mut::<PlayerStats>(entity)?;
            pstats.money += BONUS_REWARD;
            pstats.last_bonus_at = now;
            (pstats.money, pstats.creds)
        };
        if let Some(mut f) = ecs.get_mut::<PlayerFlags>(entity) {
            f.dirty = true;
        }
        Some(ClaimOutcome::Claimed { money, creds })
    });

    match outcome.flatten() {
        Some(ClaimOutcome::Claimed {
            money: new_money,
            creds,
        }) => {
            send_u_packet(tx, "P$", &money(new_money, creds).1);
            send_u_packet(tx, "DR", b"0");
            let hours = BONUS_COOLDOWN_SECS / 3600;
            crate::net::session::social::commands::send_ok(
                tx,
                "Бонус",
                &format!("Вы получили {BONUS_REWARD}$!\nВозвращайтесь через {hours} часов."),
            );
        }
        Some(ClaimOutcome::NotReady { remaining }) => {
            let hours = remaining / 3600;
            let mins = (remaining % 3600) / 60;
            crate::net::session::social::commands::send_ok(
                tx,
                "Бонус",
                &format!("Бонус ещё не готов.\nПриходите через {hours}ч {mins}м."),
            );
        }
        None => {}
    }
}
