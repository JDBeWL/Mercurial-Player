//! QQ 音乐 QRC(逐字歌词)解密与解析

#![allow(clippy::unreadable_literal, clippy::identity_op)]

use flate2::read::ZlibDecoder;
use std::fmt::Write as _;
use std::io::Read;

/// 8 字节密钥
const KEY_1: [u8; 8] = *b"!@#)(*$%";
const KEY_2: [u8; 8] = *b"123ZXC!@";
const KEY_3: [u8; 8] = *b"!@#)(NHL";

/// S 盒
const SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12,
        11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9,
        1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1,
        10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15,
        4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5,
        14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6,
        9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2,
        12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10,
        10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15,
        10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14,
        2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13,
        14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5,
        15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5,
        12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4,
        10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6,
        11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10,
        8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// 置换选择 1 - C 部分(0 基位索引)
const KEY_PERM_C: [u8; 28] = [
    56, 48, 40, 32, 24, 16, 8, 0, 57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59,
    51, 43, 35,
];

/// 置换选择 1 - D 部分(0 基位索引)
const KEY_PERM_D: [u8; 28] = [
    62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 60, 52, 44, 36, 28, 20, 12, 4,
    27, 19, 11, 3,
];

/// 置换选择 2(PC-2)
const KEY_COMPRESSION: [u8; 48] = [
    13, 16, 10, 23, 0, 4, 2, 27, 14, 5, 20, 9, 22, 18, 11, 3, 25, 7, 15, 6, 26, 19, 12, 1, 40, 51,
    30, 36, 46, 54, 29, 39, 50, 44, 32, 47, 43, 48, 38, 55, 33, 52, 45, 41, 49, 35, 28, 31,
];

/// 每轮循环左移位数
const KEY_RND_SHIFT: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

fn sbox_bit(a: u32) -> u32 {
    (a & 32) | ((a & 31) >> 1) | ((a & 1) << 4)
}

/// 初始置换:把 8 字节块按 QQ 的字节序拆成两个 32 位整数
fn initial_permutation(input: [u8; 8]) -> (u32, u32) {
    let v0: u32 = input[0] as u32
        | (input[1] as u32) << 8
        | (input[2] as u32) << 16
        | (input[3] as u32) << 24;
    let v1: u32 = input[4] as u32
        | (input[5] as u32) << 8
        | (input[6] as u32) << 16
        | (input[7] as u32) << 24;

    let s0: u32 = ((v1 >> 6) & 1) << 31
        | ((v1 >> 14) & 1) << 30
        | ((v1 >> 22) & 1) << 29
        | ((v1 >> 30) & 1) << 28
        | ((v0 >> 6) & 1) << 27
        | ((v0 >> 14) & 1) << 26
        | ((v0 >> 22) & 1) << 25
        | ((v0 >> 30) & 1) << 24
        | ((v1 >> 4) & 1) << 23
        | ((v1 >> 12) & 1) << 22
        | ((v1 >> 20) & 1) << 21
        | ((v1 >> 28) & 1) << 20
        | ((v0 >> 4) & 1) << 19
        | ((v0 >> 12) & 1) << 18
        | ((v0 >> 20) & 1) << 17
        | ((v0 >> 28) & 1) << 16
        | ((v1 >> 2) & 1) << 15
        | ((v1 >> 10) & 1) << 14
        | ((v1 >> 18) & 1) << 13
        | ((v1 >> 26) & 1) << 12
        | ((v0 >> 2) & 1) << 11
        | ((v0 >> 10) & 1) << 10
        | ((v0 >> 18) & 1) << 9
        | ((v0 >> 26) & 1) << 8
        | ((v1 >> 0) & 1) << 7
        | ((v1 >> 8) & 1) << 6
        | ((v1 >> 16) & 1) << 5
        | ((v1 >> 24) & 1) << 4
        | ((v0 >> 0) & 1) << 3
        | ((v0 >> 8) & 1) << 2
        | ((v0 >> 16) & 1) << 1
        | ((v0 >> 24) & 1);
    let s1: u32 = ((v1 >> 7) & 1) << 31
        | ((v1 >> 15) & 1) << 30
        | ((v1 >> 23) & 1) << 29
        | ((v1 >> 31) & 1) << 28
        | ((v0 >> 7) & 1) << 27
        | ((v0 >> 15) & 1) << 26
        | ((v0 >> 23) & 1) << 25
        | ((v0 >> 31) & 1) << 24
        | ((v1 >> 5) & 1) << 23
        | ((v1 >> 13) & 1) << 22
        | ((v1 >> 21) & 1) << 21
        | ((v1 >> 29) & 1) << 20
        | ((v0 >> 5) & 1) << 19
        | ((v0 >> 13) & 1) << 18
        | ((v0 >> 21) & 1) << 17
        | ((v0 >> 29) & 1) << 16
        | ((v1 >> 3) & 1) << 15
        | ((v1 >> 11) & 1) << 14
        | ((v1 >> 19) & 1) << 13
        | ((v1 >> 27) & 1) << 12
        | ((v0 >> 3) & 1) << 11
        | ((v0 >> 11) & 1) << 10
        | ((v0 >> 19) & 1) << 9
        | ((v0 >> 27) & 1) << 8
        | ((v1 >> 1) & 1) << 7
        | ((v1 >> 9) & 1) << 6
        | ((v1 >> 17) & 1) << 5
        | ((v1 >> 25) & 1) << 4
        | ((v0 >> 1) & 1) << 3
        | ((v0 >> 9) & 1) << 2
        | ((v0 >> 17) & 1) << 1
        | ((v0 >> 25) & 1);

    (s0, s1)
}

/// 逆初始置换
fn inverse_permutation(s0: u32, s1: u32) -> [u8; 8] {
    let mut data = [0u8; 8];
    data[0] = (((s1 >> 27) & 1) << 7
        | ((s0 >> 27) & 1) << 6
        | ((s1 >> 19) & 1) << 5
        | ((s0 >> 19) & 1) << 4
        | ((s1 >> 11) & 1) << 3
        | ((s0 >> 11) & 1) << 2
        | ((s1 >> 3) & 1) << 1
        | ((s0 >> 3) & 1)) as u8;
    data[1] = (((s1 >> 26) & 1) << 7
        | ((s0 >> 26) & 1) << 6
        | ((s1 >> 18) & 1) << 5
        | ((s0 >> 18) & 1) << 4
        | ((s1 >> 10) & 1) << 3
        | ((s0 >> 10) & 1) << 2
        | ((s1 >> 2) & 1) << 1
        | ((s0 >> 2) & 1)) as u8;
    data[2] = (((s1 >> 25) & 1) << 7
        | ((s0 >> 25) & 1) << 6
        | ((s1 >> 17) & 1) << 5
        | ((s0 >> 17) & 1) << 4
        | ((s1 >> 9) & 1) << 3
        | ((s0 >> 9) & 1) << 2
        | ((s1 >> 1) & 1) << 1
        | ((s0 >> 1) & 1)) as u8;
    data[3] = (((s1 >> 24) & 1) << 7
        | ((s0 >> 24) & 1) << 6
        | ((s1 >> 16) & 1) << 5
        | ((s0 >> 16) & 1) << 4
        | ((s1 >> 8) & 1) << 3
        | ((s0 >> 8) & 1) << 2
        | ((s1 >> 0) & 1) << 1
        | ((s0 >> 0) & 1)) as u8;
    data[4] = (((s1 >> 31) & 1) << 7
        | ((s0 >> 31) & 1) << 6
        | ((s1 >> 23) & 1) << 5
        | ((s0 >> 23) & 1) << 4
        | ((s1 >> 15) & 1) << 3
        | ((s0 >> 15) & 1) << 2
        | ((s1 >> 7) & 1) << 1
        | ((s0 >> 7) & 1)) as u8;
    data[5] = (((s1 >> 30) & 1) << 7
        | ((s0 >> 30) & 1) << 6
        | ((s1 >> 22) & 1) << 5
        | ((s0 >> 22) & 1) << 4
        | ((s1 >> 14) & 1) << 3
        | ((s0 >> 14) & 1) << 2
        | ((s1 >> 6) & 1) << 1
        | ((s0 >> 6) & 1)) as u8;
    data[6] = (((s1 >> 29) & 1) << 7
        | ((s0 >> 29) & 1) << 6
        | ((s1 >> 21) & 1) << 5
        | ((s0 >> 21) & 1) << 4
        | ((s1 >> 13) & 1) << 3
        | ((s0 >> 13) & 1) << 2
        | ((s1 >> 5) & 1) << 1
        | ((s0 >> 5) & 1)) as u8;
    data[7] = (((s1 >> 28) & 1) << 7
        | ((s0 >> 28) & 1) << 6
        | ((s1 >> 20) & 1) << 5
        | ((s0 >> 20) & 1) << 4
        | ((s1 >> 12) & 1) << 3
        | ((s0 >> 12) & 1) << 2
        | ((s1 >> 4) & 1) << 1
        | ((s0 >> 4) & 1)) as u8;
    data
}

/// Feistel
fn feistel(state: u32, key: [u8; 6]) -> u32 {
    let t1: u32 = ((state & 1) << 31)
        | ((state & 0xF8000000) >> 1)
        | ((state & 0x1F800000) >> 3)
        | ((state & 0x01F80000) >> 5)
        | ((state & 0x001F8000) >> 7);
    let t2: u32 = ((state & 0x0001F800) << 15)
        | ((state & 0x00001F80) << 13)
        | ((state & 0x000001F8) << 11)
        | ((state & 0x0000001F) << 9)
        | ((state & 0x80000000) >> 23);

    let k0: u32 = ((t1 >> 24) & 0xFF) ^ key[0] as u32;
    let k1: u32 = ((t1 >> 16) & 0xFF) ^ key[1] as u32;
    let k2: u32 = ((t1 >> 8) & 0xFF) ^ key[2] as u32;
    let k3: u32 = ((t2 >> 24) & 0xFF) ^ key[3] as u32;
    let k4: u32 = ((t2 >> 16) & 0xFF) ^ key[4] as u32;
    let k5: u32 = ((t2 >> 8) & 0xFF) ^ key[5] as u32;

    let state: u32 = ((SBOX[0][sbox_bit(k0 >> 2) as usize] as u32) << 28)
        | ((SBOX[1][sbox_bit(((k0 & 0x03) << 4) | (k1 >> 4)) as usize] as u32) << 24)
        | ((SBOX[2][sbox_bit(((k1 & 0x0F) << 2) | (k2 >> 6)) as usize] as u32) << 20)
        | ((SBOX[3][sbox_bit(k2 & 0x3F) as usize] as u32) << 16)
        | ((SBOX[4][sbox_bit(k3 >> 2) as usize] as u32) << 12)
        | ((SBOX[5][sbox_bit(((k3 & 0x03) << 4) | (k4 >> 4)) as usize] as u32) << 8)
        | ((SBOX[6][sbox_bit(((k4 & 0x0F) << 2) | (k5 >> 6)) as usize] as u32) << 4)
        | (SBOX[7][sbox_bit(k5 & 0x3F) as usize] as u32);

    ((state >> 16) & 1) << 31
        | ((state >> 25) & 1) << 30
        | ((state >> 12) & 1) << 29
        | ((state >> 11) & 1) << 28
        | ((state >> 3) & 1) << 27
        | ((state >> 20) & 1) << 26
        | ((state >> 4) & 1) << 25
        | ((state >> 15) & 1) << 24
        | ((state >> 31) & 1) << 23
        | ((state >> 17) & 1) << 22
        | ((state >> 9) & 1) << 21
        | ((state >> 6) & 1) << 20
        | ((state >> 27) & 1) << 19
        | ((state >> 14) & 1) << 18
        | ((state >> 1) & 1) << 17
        | ((state >> 22) & 1) << 16
        | ((state >> 30) & 1) << 15
        | ((state >> 24) & 1) << 14
        | ((state >> 8) & 1) << 13
        | ((state >> 18) & 1) << 12
        | ((state >> 0) & 1) << 11
        | ((state >> 5) & 1) << 10
        | ((state >> 29) & 1) << 9
        | ((state >> 23) & 1) << 8
        | ((state >> 13) & 1) << 7
        | ((state >> 19) & 1) << 6
        | ((state >> 2) & 1) << 5
        | ((state >> 26) & 1) << 4
        | ((state >> 10) & 1) << 3
        | ((state >> 21) & 1) << 2
        | ((state >> 28) & 1) << 1
        | ((state >> 7) & 1)
}

/// 单个 8 字节块的加解密
fn crypt_block(input: [u8; 8], key: &[[u8; 6]; 16]) -> [u8; 8] {
    let (mut s0, mut s1) = initial_permutation(input);

    for subkey in key.iter().take(15) {
        let previous_s1 = s1;
        s1 = feistel(s1, *subkey) ^ s0;
        s0 = previous_s1;
    }
    s0 ^= feistel(s1, key[15]);

    inverse_permutation(s0, s1)
}

/// 8 → 16 轮密钥调度
fn key_schedule(key: [u8; 8], decrypt: bool) -> [[u8; 6]; 16] {
    let mut schedule = [[0u8; 6]; 16];

    let v0: u32 =
        key[0] as u32 | (key[1] as u32) << 8 | (key[2] as u32) << 16 | (key[3] as u32) << 24;
    let v1: u32 =
        key[4] as u32 | (key[5] as u32) << 8 | (key[6] as u32) << 16 | (key[7] as u32) << 24;

    let mut c: u32 = 0;
    for (i, pos) in KEY_PERM_C.iter().enumerate() {
        let pos = *pos as u32;
        let bit = if pos < 32 {
            (v0 >> (31 - pos)) & 1
        } else {
            (v1 >> (63 - pos)) & 1
        };
        c |= bit << (31 - i as u32);
    }
    let mut d: u32 = 0;
    for (i, pos) in KEY_PERM_D.iter().enumerate() {
        let pos = *pos as u32;
        let bit = if pos < 32 {
            (v0 >> (31 - pos)) & 1
        } else {
            (v1 >> (63 - pos)) & 1
        };
        d |= bit << (31 - i as u32);
    }

    for (i, shift) in KEY_RND_SHIFT.iter().enumerate() {
        c = ((c << shift) | (c >> (28 - shift))) & 0xFFFF_FFF0;
        d = ((d << shift) | (d >> (28 - shift))) & 0xFFFF_FFF0;

        let to_gen = if decrypt { 15 - i } else { i };

        for (j, pos) in KEY_COMPRESSION.iter().take(24).enumerate() {
            let bit = (c >> (31 - *pos as u32)) & 1;
            schedule[to_gen][j / 8] |= (bit << (7 - (j % 8))) as u8;
        }
        for (k, pos) in KEY_COMPRESSION.iter().skip(24).enumerate() {
            let j = k + 24;
            let bit = (d >> (31 - (*pos as u32 - 27))) & 1;
            schedule[to_gen][j / 8] |= (bit << (7 - (j % 8))) as u8;
        }
    }

    schedule
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).ok()?;
    Some(out)
}

/// 十六进制 → 3DES → zlib → UTF-8
pub(crate) fn decrypt_payload(payload: &str) -> Option<String> {
    let trimmed = payload.trim();
    let Some(bytes) = hex_decode(trimmed) else {
        return Some(trimmed.to_string());
    };
    if bytes.is_empty() || !bytes.len().is_multiple_of(8) {
        return None;
    }

    // 解密顺序 D(K3) → E(K2) → D(K1)
    let schedules = [
        key_schedule(KEY_3, true),
        key_schedule(KEY_2, false),
        key_schedule(KEY_1, true),
    ];

    let mut decrypted = Vec::with_capacity(bytes.len());
    for chunk in bytes.as_chunks::<8>().0 {
        let mut block = *chunk;
        for schedule in &schedules {
            block = crypt_block(block, schedule);
        }
        decrypted.extend_from_slice(&block);
    }

    let text = inflate(&decrypted)?;
    let text = String::from_utf8_lossy(&text);
    Some(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

fn tag_cdata(xml: &str, tag: &str) -> Option<String> {
    let opener = format!("<{tag}");
    let mut from = 0usize;
    while let Some(rel) = xml[from..].find(&opener) {
        let start = from + rel;
        let following = xml[start + opener.len()..].chars().next();
        if matches!(following, Some(c) if !c.is_ascii_alphanumeric()) {
            let after = &xml[start..];
            let cdata = after.find("<![CDATA[")? + "<![CDATA[".len();
            let rest = &after[cdata..];
            let end = rest.find("]]>")?;
            return Some(rest[..end].to_string());
        }
        from = start + opener.len();
    }
    None
}

fn unescape_xml(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#10;", "\n")
        .replace("&#13;", "\r")
        .replace("&amp;", "&")
}

fn extract_lyric_content(xml: &str) -> String {
    if let Some((_, rest)) = xml.split_once("LyricContent=\"") {
        if let Some((value, _)) = rest.split_once('"') {
            return unescape_xml(value);
        }
    }
    xml.trim().to_string()
}

/// 毫秒 → LRC 时间戳 `mm:ss.cc`
fn format_lrc_time(ms: i64) -> String {
    let total = ms.max(0);
    format!(
        "{:02}:{:02}.{:02}",
        total / 60_000,
        (total % 60_000) / 1000,
        (total % 1000) / 10
    )
}

/// 解析
fn parse_qrc_words(body: &str) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    let mut pos = 0usize;

    while pos < body.len() {
        // 括号内必须是 `数字,数字`
        let mut search = pos;
        let mut marker: Option<(usize, usize, i64)> = None;
        while let Some(rel) = body[search..].find('(') {
            let open = search + rel;
            let Some(close_rel) = body[open + 1..].find(')') else {
                break;
            };
            let inside = &body[open + 1..open + 1 + close_rel];
            let valid = inside.split_once(',').filter(|(start, duration)| {
                !start.is_empty()
                    && !duration.is_empty()
                    && start.bytes().all(|c| c.is_ascii_digit())
                    && duration.bytes().all(|c| c.is_ascii_digit())
            });
            if let Some((start, _)) = valid {
                if let Ok(start_ms) = start.parse::<i64>() {
                    marker = Some((open, open + 1 + close_rel + 1, start_ms));
                    break;
                }
            }
            search = open + 1;
        }

        let Some((open, next_pos, start_ms)) = marker else {
            break;
        };
        let text = &body[pos..open];
        if !text.is_empty() && text.trim() != "/" {
            out.push((start_ms, text.to_string()));
        }
        pos = next_pos;
    }

    // 末尾没有时间标记的文本合并进上一段
    if pos < body.len() {
        let tail = &body[pos..];
        if !tail.is_empty() && tail.trim() != "/" {
            match out.last_mut() {
                Some(last) => last.1.push_str(tail),
                None => out.push((0, tail.to_string())),
            }
        }
    }

    out
}

/// 复用 `<mm:ss.xx>` 解析与 ASS 生成
pub(crate) fn to_marked_lrc(content: &str) -> String {
    let mut lines: Vec<String> = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("[kana:") {
            continue;
        }
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(bracket) = rest.find(']') else {
            continue;
        };
        let header = &rest[..bracket];
        let body = &rest[bracket + 1..];

        // 只有 "起始,时长" 全为数字的行才是逐字行,其余是 [ti:]/[ar:]/[offset:0] 元信息
        let timed = header.split_once(',').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())
        });
        if !timed {
            lines.push(line.to_string());
            continue;
        }

        let start_ms = header
            .split_once(',')
            .and_then(|(a, _)| a.parse::<i64>().ok())
            .unwrap_or(0);

        let mut marked = format!("[{}]", format_lrc_time(start_ms));
        let mut words = 0usize;
        for (word_start, text) in parse_qrc_words(body) {
            let _ = write!(marked, "<{}>{}", format_lrc_time(word_start), text);
            words += 1;
        }
        if words > 0 {
            lines.push(marked);
        } else {
            // 整行没有时间标记:退化成普通 LRC 行,不要整行丢掉
            let plain = body.trim();
            if !plain.is_empty() && plain != "/" {
                lines.push(format!("[{}]{}", format_lrc_time(start_ms), plain));
            }
        }
    }

    lines.join("\n")
}

/// 清理普通 LRC(翻译节点):丢掉 `//` 占位行、`[kana:…]` 与空行
pub(crate) fn clean_plain_lrc(text: &str) -> String {
    text.lines()
        .map(|line| line.trim())
        .filter(|line| {
            if line.is_empty() || line.starts_with("[kana:") {
                return false;
            }
            let body = line
                .rsplit_once(']')
                .map(|(_, rest)| rest.trim())
                .unwrap_or(*line);
            body != "//"
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 解析出的三段内容
pub(crate) struct QrcLyrics {
    /// 逐字原文(生成 ASS / 干净 LRC)
    pub(crate) lyric: String,
    /// 翻译(普通 LRC)
    pub(crate) trans: String,
    /// 罗马音(带逐字标记,调用方按需去掉标记)
    pub(crate) roma: String,
}

/// 解析 XML
pub(crate) fn extract(xml: &str) -> Option<QrcLyrics> {
    let lyric = tag_cdata(xml, "content")
        .and_then(|payload| decrypt_payload(&payload))
        .map(|text| to_marked_lrc(&extract_lyric_content(&text)))
        .filter(|text| !text.is_empty())?;

    let trans = tag_cdata(xml, "contentts")
        .and_then(|payload| decrypt_payload(&payload))
        .map(|text| clean_plain_lrc(&extract_lyric_content(&text)))
        .unwrap_or_default();

    let roma = tag_cdata(xml, "contentroma")
        .and_then(|payload| decrypt_payload(&payload))
        .map(|text| to_marked_lrc(&extract_lyric_content(&text)))
        .unwrap_or_default();

    Some(QrcLyrics { lyric, trans, roma })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC_HEX: &str = r"930431620f2b19298e4c360e923e291cc74aef291e6d2660abf0d5086377e5b83b8555a8d4fe5c8d6cf0314115c2d067dd243566240e8aaea7469075c5414c25f4539474e8364bfff0699f4e5b43f677e119a37adac549d96dadce9489d9e28c8547cbe8b4abb2d79ac8777f24bf4ff593fd41b93a288b09c9512f03e54fff9d1cbc05ea9b34237d698a02fc406aa76d2430df201adc7ac3765c337491ce2a2e8627cc5fc21d3479acc54061c8b70314c05dfe647792484f6da1c6066ced6b1ed4042a71732e78ea9c66a054112028d1136ec1195b964bd94eb112a70ef4ef8e1e7e62ceff8a506140d8ecd9598ceee1a16f8c4b25f0b15a8d40517d30471b3694810aa0eb335484db68f82f2243739b3c0329f2da1a7c5bebf3a3e065726302e14f4956b26afaba";

    #[test]
    fn 解密合成向量并解析逐字行() {
        let text = decrypt_payload(SYNTHETIC_HEX).expect("应能解密");
        assert!(text.contains("测试曲"), "解密结果应含曲名元信息: {text}");
        assert!(!text.contains('\u{feff}'), "应去掉 UTF-8 BOM");

        let content = extract_lyric_content(&text);
        assert!(
            content.contains("[ar:测试歌手]"),
            "应还原 &#10; 转义后的换行"
        );

        let marked = to_marked_lrc(&content);
        assert!(marked.contains("[ti:测试曲]"));
        assert!(marked.contains("[ar:测试歌手]"));
        assert!(!marked.contains("kana"), "逐字假名应被丢弃: {marked}");
        assert!(
            marked.contains("[00:01.54]<00:01.54>梦<00:01.92>想<00:02.14> "),
            "时间戳需与文本配对、空格段需保留, 实际: {marked}"
        );
        assert!(
            marked.contains("[00:02.88]<00:02.88>Shape<00:03.35> <00:03.45>of"),
            "英文的空格段需保留, 实际: {marked}"
        );
        assert!(!marked.contains('/'), "换行占位不应进入文本: {marked}");
    }

    #[test]
    fn 明文载荷原样返回() {
        let plain = "[ti:x]\n[ar:y]\n[00:01.54]如果只是一场梦";
        assert_eq!(decrypt_payload(plain), Some(plain.to_string()));
        assert_eq!(decrypt_payload("  [ti:x]  "), Some("[ti:x]".to_string()));
    }

    #[test]
    fn 长度不是八的倍数时返回none() {
        // 合法十六进制但不是 8 字节的整数倍
        assert_eq!(decrypt_payload("AABBCC"), None);
    }

    #[test]
    fn 提取lyric_content与反转义() {
        let xml = r#"<?xml version="1.0"?><Lyric_1 LyricContent="[ti:标题]&#10;[ar:歌手]&quot;引用&quot;" />"#;
        assert_eq!(extract_lyric_content(xml), "[ti:标题]\n[ar:歌手]\"引用\"");
        // 没有该属性时原样返回
        assert_eq!(extract_lyric_content("[ti:裸文本]"), "[ti:裸文本]");
    }

    #[test]
    fn 清理翻译节点占位行() {
        let raw = "[ti:x]\n[ar:y]\n[00:00.00]//\n[00:01.54]如果只是一场梦\n[00:02.88]那该有多好";
        let cleaned = clean_plain_lrc(raw);
        assert!(cleaned.contains("[00:01.54]如果只是一场梦"));
        assert!(!cleaned.contains("//"));
    }

    #[test]
    fn tag_cdata_不误命中同前缀标签() {
        let xml = r#"<contentts x="1"><![CDATA[译文]]></contentts><content y="2"><![CDATA[正文]]></content>"#;
        assert_eq!(tag_cdata(xml, "content").as_deref(), Some("正文"));
        assert_eq!(tag_cdata(xml, "contentts").as_deref(), Some("译文"));
        assert_eq!(tag_cdata(xml, "contentroma"), None);
    }

    #[test]
    fn extract_缺少正文节点返回none() {
        assert!(extract("<contentts><![CDATA[[ti:x]]]></contentts>").is_none());
    }

    #[test]
    fn 行首文本用其后的时间标记() {
        // `Lemon - ` 在第一个时间标记之前,属于该标记的时间点
        let content = "[1547,1152]Lemon - (1547,33)米(1580,66)津(1646,33)";
        let marked = to_marked_lrc(content);
        assert_eq!(
            marked,
            "[00:01.54]<00:01.54>Lemon - <00:01.58>米<00:01.64>津"
        );
    }

    /// 歌词文本里出现字面量 `(` 时不能当作时间标记,否则整行会被截断
    #[test]
    fn 文本中的括号不截断整行() {
        let content = "[0,529]Lemon - (0,33)米津玄師 ( (232,33)よね(265,33)づ";
        let marked = to_marked_lrc(content);
        assert_eq!(
            marked,
            "[00:00.00]<00:00.00>Lemon - <00:00.23>米津玄師 ( <00:00.26>よねづ"
        );
    }

    #[test]
    fn 保留英文歌词的空格段() {
        let content = "[0,2380]Shape(0,476) (476,95)of(571,190) (761,95)You(856,286)";
        let marked = to_marked_lrc(content);
        assert_eq!(
            marked,
            "[00:00.00]<00:00.00>Shape<00:00.47> <00:00.57>of<00:00.76> <00:00.85>You"
        );
        // 空格段必须原样落在文本里(而不是被 trim 掉)
        assert!(marked.contains("Shape<00:00.47> "), "实际: {marked}");
        assert!(marked.contains("of<00:00.76> "), "实际: {marked}");
    }
}
