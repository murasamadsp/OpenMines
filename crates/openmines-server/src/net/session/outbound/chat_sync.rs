/// Разбор тега приватного канала `_a_b` → `(a, b)` (id игроков, i32).
pub fn parse_private_tag(tag: &str) -> Option<(i32, i32)> {
    let rest = tag.strip_prefix('_')?;
    let mut it = rest.split('_');
    let a: i32 = it.next()?.parse().ok()?;
    let b: i32 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b))
}
