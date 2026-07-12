use binrw::BinWrite;
use bytes::{BufMut, BytesMut};

/// Build a complete HB packet payload from sub-packets.
#[must_use]
pub fn hb_bundle(sub_packets: &[Vec<u8>]) -> (&'static str, Vec<u8>) {
    let total_len: usize = sub_packets.iter().map(std::vec::Vec::len).sum();
    let mut buf = BytesMut::with_capacity(total_len);
    for packet in sub_packets {
        buf.put_slice(packet);
    }
    ("HB", buf.to_vec())
}

fn write_slice<'a, T, W>(
    slice: &&[T],
    writer: &mut W,
    endian: binrw::Endian,
    _args: (),
) -> binrw::BinResult<()>
where
    T: binrw::BinWrite<Args<'a> = ()>,
    W: std::io::Write + std::io::Seek,
{
    for item in *slice {
        item.write_options(writer, endian, ())?;
    }
    Ok(())
}

fn write_hb_packet(
    write: impl FnOnce(&mut std::io::Cursor<Vec<u8>>) -> binrw::BinResult<()>,
) -> Vec<u8> {
    let mut writer = std::io::Cursor::new(Vec::new());
    write(&mut writer).expect("writing HB packet to Vec cannot fail");
    writer.into_inner()
}

#[derive(BinWrite)]
#[bw(little)]
struct HbMap<'a> {
    tag: u8,
    width: u8,
    height: u8,
    x: u16,
    y: u16,
    #[bw(write_with = write_slice)]
    cells: &'a [u8],
}

/// HB sub-packet: Map chunk (type `M`).
#[must_use]
pub fn hb_map(x: u16, y: u16, width: u8, height: u8, cells: &[u8]) -> Vec<u8> {
    let packet = HbMap {
        tag: b'M',
        width,
        height,
        x,
        y,
        cells,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBot {
    tag: u8,
    dir: u8,
    skin: u8,
    tail: u8,
    id: u16,
    x: u16,
    y: u16,
    clan_id: u16,
}

/// HB sub-packet: Bot (type `X`).
#[must_use]
pub fn hb_bot(id: u16, x: u16, y: u16, dir: u8, skin: u8, clan_id: u16, tail: u8) -> Vec<u8> {
    let packet = HbBot {
        tag: b'X',
        dir,
        skin,
        tail,
        id,
        x,
        y,
        clan_id,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbPackItem {
    code: u8,
    x: u16,
    y: u16,
    clan_id: u8,
    zero: u8,
    off: u8,
}

#[derive(BinWrite)]
#[bw(little)]
struct HbPacks {
    tag: u8,
    block_pos: i32,
    count: u16,
    packs: Vec<HbPackItem>,
}

/// HB sub-packet: Pack/building (type `O`).
///
/// The client reads clan as u16 from bytes 5 and 6 while the reference encoder
/// writes the clan byte at 5 and leaves byte 6 zero. Keep that asymmetric layout.
#[must_use]
pub fn hb_packs(block_pos: i32, packs: &[(u8, u16, u16, u8, u8)]) -> Vec<u8> {
    let Ok(count) = u16::try_from(packs.len()) else {
        return vec![];
    };
    let items = packs
        .iter()
        .map(|&(code, x, y, clan_id, off)| HbPackItem {
            code,
            x,
            y,
            clan_id,
            zero: 0,
            off,
        })
        .collect();
    let packet = HbPacks {
        tag: b'O',
        block_pos,
        count,
        packs: items,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbFx {
    tag: u8,
    fx_type: u8,
    x: u16,
    y: u16,
}

/// HB sub-packet: FX effect (type `F`).
#[must_use]
pub fn hb_fx(x: u16, y: u16, fx_type: u8) -> Vec<u8> {
    let packet = HbFx {
        tag: b'F',
        fx_type,
        x,
        y,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBotDel {
    tag: u8,
    id: u16,
}

/// HB sub-packet: Delete object/bot (type `L`).
#[must_use]
pub fn hb_bot_del(id: u16) -> Vec<u8> {
    let packet = HbBotDel { tag: b'L', id };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBotLeaveBlock {
    tag: u8,
    id: u16,
    block_pos: i32,
}

/// HB sub-packet: Bot leave block (type `S`).
#[must_use]
pub fn hb_bot_leave_block(id: u16, block_pos: i32) -> Vec<u8> {
    let packet = HbBotLeaveBlock {
        tag: b'S',
        id,
        block_pos,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbDirectedFx {
    tag: u8,
    fx: u8,
    dir: u16,
    color: u8,
    x: u16,
    y: u16,
    bot_id: u16,
}

/// HB sub-packet: Directed FX (type `D`).
#[must_use]
pub fn hb_directed_fx(bot_id: u16, x: u16, y: u16, fx: u8, dir: u16, color: u8) -> Vec<u8> {
    let packet = HbDirectedFx {
        tag: b'D',
        fx,
        dir,
        color,
        x,
        y,
        bot_id,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[must_use]
pub fn hb_dig_fx(bot_id: u16, x: u16, y: u16, dir: u16) -> Vec<u8> {
    hb_directed_fx(bot_id, x, y, 0, dir, 0)
}

#[must_use]
pub fn hb_world_blast_fx(x: u16, y: u16, radius_dir: u16, color: u8) -> Vec<u8> {
    hb_directed_fx(0, x, y, 1, radius_dir, color)
}

/// The client expands values `201..=255` into larger labels; values above `255` are not valid codes.
#[must_use]
pub fn hb_crystal_mine_fx(bot_id: u16, x: u16, y: u16, amount: i64, color: u8) -> Vec<u8> {
    let visual_amount = u16::try_from(amount.clamp(0, 255)).unwrap_or_default();
    hb_directed_fx(bot_id, x, y, 2, visual_amount, color)
}

#[must_use]
pub fn hb_heal_fx(bot_id: u16) -> Vec<u8> {
    hb_directed_fx(bot_id, 0, 0, 5, 0, 0)
}

#[must_use]
pub fn hb_hurt_fx(bot_id: u16) -> Vec<u8> {
    hb_directed_fx(bot_id, 0, 0, 6, 0, 0)
}

#[must_use]
pub fn hb_gun_shot_fx(bot_id: u16, x: u16, y: u16) -> Vec<u8> {
    hb_directed_fx(bot_id, x, y, 7, 1, 0)
}

#[derive(BinWrite)]
#[bw(little)]
struct HbChat<'a> {
    tag: u8,
    bot_id: u16,
    x: u16,
    y: u16,
    text_len: u16,
    #[bw(write_with = write_slice)]
    text: &'a [u8],
}

/// HB sub-packet: Chat bubble (type `C`).
#[must_use]
pub fn hb_chat(bot_id: u16, x: u16, y: u16, text: &str) -> Vec<u8> {
    let text_bytes = text.as_bytes();
    let Ok(text_len) = u16::try_from(text_bytes.len()) else {
        return vec![];
    };
    let packet = HbChat {
        tag: b'C',
        bot_id,
        x,
        y,
        text_len,
        text: text_bytes,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbBotsList<'a> {
    tag: u8,
    count: u16,
    #[bw(write_with = write_slice)]
    bot_ids: &'a [u16],
}

/// HB sub-packet: Bot list (type `B`).
#[must_use]
pub fn hb_bots_list(bot_ids: &[u16]) -> Vec<u8> {
    let Ok(count) = u16::try_from(bot_ids.len()) else {
        return vec![];
    };
    let packet = HbBotsList {
        tag: b'B',
        count,
        bot_ids,
    };
    write_hb_packet(|writer| packet.write(writer))
}

#[derive(BinWrite)]
#[bw(little)]
struct HbGun<'a> {
    tag: u8,
    amount: u8,
    color: u8,
    x: u16,
    y: u16,
    #[bw(write_with = write_slice)]
    bot_ids: &'a [u16],
}

/// HB sub-packet: Gun/shot (type `Z`).
#[must_use]
pub fn hb_gun(x: u16, y: u16, color: u8, bot_ids: &[u16]) -> Vec<u8> {
    let Ok(amount) = u8::try_from(bot_ids.len()) else {
        return vec![];
    };
    let packet = HbGun {
        tag: b'Z',
        amount,
        color,
        x,
        y,
        bot_ids,
    };
    write_hb_packet(|writer| packet.write(writer))
}

/// HB sub-packet: single cell update, wrapping `hb_map` with 1x1 dimensions.
#[must_use]
pub fn hb_cell(x: u16, y: u16, cell: u8) -> Vec<u8> {
    hb_map(x, y, 1, 1, &[cell])
}
