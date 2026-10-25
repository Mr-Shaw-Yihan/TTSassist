//! Sec-MS-GEC v2 令牌生成。
//!
//! 2026-10 微软对 Edge Read-Aloud 的握手校验升级到 v2 令牌（分阶段上线，v1 令牌一律
//! 403，见 rany2/edge-tts#490；上游 kothok-edge-tts 0.2.10 未跟进）。本模块把 v2 算法
//! 移植自 MIT 的 rany2/edge-tts 7.3.0 `drm.py`：CS64 摘要 + 调整版 SHA-256，
//! 输出格式 `<16 hex 大写>-<64 hex 大写>`。
//!
//! 与 drm.py 的两处已知差异都是纯实现优化，已验证等价：
//! - `_sha256_rounds` 的「角色轮转变量名」8 步展开 ≡ 标准 SHA-256 轮循环（200 组随机
//!   输入逐位比对通过），这里直接写标准循环；
//! - 常量表（含调整过的 round constant / 初始态 / schedule）由 drm.py 的常量机器生成，
//!   无手抄。
//!
//! 正确性由 `v2_matches_python_reference` 差分测试钉死——参考值由 Python 实现（已通过
//! 服务端 101 握手实测）对固定时间戳生成，任何移植笔误都会被它抓住。

use super::auth::TRUSTED_CLIENT_TOKEN;

const U32_MASK: u64 = 0xFFFF_FFFF;

// ── 常量表（机器生成自 drm.py，勿手改）────────────────────────────

include!("token_v2_consts.rs");

// ── 移植的算法函数 ────────────────────────────────────────────────

/// 32 位值循环左移 16 位（drm.py `_rol16`）。
fn rol16(value: u32) -> u32 {
    value.rotate_left(16)
}

/// 小端 u32 字列表，补零到偶数个（drm.py `_words_le4_even`）。
fn words_le4_even(data: &[u8]) -> Vec<u32> {
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat(0u8).take((4 - padded.len() % 4) % 4));
    let mut words: Vec<u32> = padded
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("4 bytes")))
        .collect();
    if words.len() % 2 != 0 {
        words.push(0);
    }
    words
}

/// 调整版小 sigma0（drm.py `_sigma0`，比标准 SHA-256 多两个截断旋转项）。
fn sigma0(value: u64) -> u64 {
    ((value >> 7) ^ (value << 25) ^ (value >> 18) ^ (value << 14) ^ (value >> 3)) & U32_MASK
}

/// 调整版小 sigma1（drm.py `_sigma1`）。
fn sigma1(value: u64) -> u64 {
    ((value >> 17) ^ (value << 15) ^ (value >> 19) ^ (value << 13) ^ (value >> 10)) & U32_MASK
}

/// 标准形态的 SHA-256 轮循环（与 drm.py 的角色轮转变量名展开逐位等价，已实测验证）。
/// `first_round == 14` 时使用带调整的块常量表（供 `_sha256_init` 从第 14 轮续跑）。
fn sha256_rounds(state: &[u32; 8], words: &[u64], first_round: usize) -> [u32; 8] {
    let constants: &[u32] = if first_round == 14 {
        &SHA256_BLOCK_CONSTANTS
    } else {
        &SHA256_ROUND_CONSTANTS
    };
    let mut v = *state;
    for index in first_round..64 {
        let a = v[0] as u64;
        let b = v[1] as u64;
        let c = v[2] as u64;
        let d = v[3] as u64;
        let e = v[4] as u64;
        let f = v[5] as u64;
        let g = v[6] as u64;
        let h = v[7] as u64;
        // 标准 t1/t2；乘 0x100000001 再右移等价 ROTR（drm.py 技巧），u64 无溢出。
        let rot_e = e.wrapping_mul(0x1_0000_0001);
        let rot_a = a.wrapping_mul(0x1_0000_0001);
        let t1 = h
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + (g ^ (e & (f ^ g)))
            + constants[index] as u64
            + words[index];
        let t2 = ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22)) + ((a & b) | (c & (a | b)));
        v[0] = ((t1 + t2) & U32_MASK) as u32;
        v[1] = a as u32;
        v[2] = b as u32;
        v[3] = c as u32;
        v[4] = ((d + t1) & U32_MASK) as u32;
        v[5] = e as u32;
        v[6] = f as u32;
        v[7] = g as u32;
    }
    v
}

/// 消息日程扩展（drm.py `_expand_sha256_schedule`，sigma 带调整旋转项）。
fn expand_sha256_schedule(words: &mut Vec<u64>, first_word: usize) {
    for index in first_word..64 {
        let x = words[index - 2];
        let y = words[index - 15];
        let sig1 = (x >> 17) ^ (x << 15) ^ (x >> 19) ^ (x << 13) ^ (x >> 10);
        let sig0 = (y >> 7) ^ (y << 25) ^ (y >> 18) ^ (y << 14) ^ (y >> 3);
        words.push((sig1 + words[index - 7] + sig0 + words[index - 16]) & U32_MASK);
    }
}

/// 用数据前 7 字节作「盐」派生初始态（drm.py `_sha256_init`，从第 14 轮续跑）。
fn sha256_init(salt: [u8; 7]) -> [u32; 8] {
    let mut words: Vec<u64> = vec![0; 14];
    words.push(u32::from_be_bytes([0, salt[0], salt[1], salt[2]]) as u64);
    words.push(u32::from_be_bytes([salt[3], salt[4], salt[5], salt[6]]) as u64);
    for index in 16..30 {
        let mut value = SHA256_SCHEDULE[index - 16] as u64;
        if index - 2 >= 14 {
            value += if index == 16 {
                sigma1(words[index - 2]) ^ SHA256_SIGMA1_ADJUSTMENT as u64
            } else {
                sigma1(words[index - 2])
            };
        }
        if index - 7 >= 14 {
            value += words[index - 7];
        }
        if index - 15 >= 14 {
            value += if index == 29 {
                sigma0(words[index - 15]) ^ SHA256_SIGMA0_ADJUSTMENT as u64
            } else {
                sigma0(words[index - 15])
            };
        }
        words.push(value & U32_MASK);
    }
    words.push(
        (sigma1(words[28]) + words[23] + sigma0(words[15]) + words[14]
            + SHA256_ADJUSTMENT as u64)
        .wrapping_sub(SHA256_ROUND_CONSTANTS[14] as u64)
            & U32_MASK,
    );
    expand_sha256_schedule(&mut words, 31);
    let working = sha256_rounds(&SHA256_STATE, &words, 14);
    let mut out = [0u32; 8];
    for (index, item) in out.iter_mut().enumerate() {
        *item = ((SHA256_INITIAL[index] as u64 + working[index] as u64) & U32_MASK) as u32;
    }
    out
}

/// 调整版 SHA-256 摘要（drm.py `_sha256_digest`，长度域固定为 57+len(data)）。
fn sha256_digest(data: &[u8]) -> [u8; 32] {
    debug_assert!(data.len() >= 7, "token salt must contain at least seven bytes");
    let length = 57usize + data.len();
    let mut padded = data[7..].to_vec();
    padded.push(0x80);
    padded.extend(std::iter::repeat(0u8).take((55i64 - length as i64).rem_euclid(64) as usize));
    padded.extend_from_slice(&(length as u64 * 8).to_be_bytes());
    let salt: [u8; 7] = data[..7].try_into().expect("7 bytes");
    let mut state = sha256_init(salt);
    for offset in (0..padded.len()).step_by(64) {
        let mut words: Vec<u64> = padded[offset..offset + 64]
            .chunks_exact(4)
            .map(|chunk| u32::from_be_bytes(chunk.try_into().expect("4 bytes")) as u64)
            .collect();
        expand_sha256_schedule(&mut words, 16);
        let working = sha256_rounds(&state, &words, 0);
        for (index, item) in state.iter_mut().enumerate() {
            *item = ((*item as u64 + working[index] as u64) & U32_MASK) as u32;
        }
    }
    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// CS64 摘要（drm.py `_cs64_digest`）。
fn cs64_digest(data: &[u8], digest: &[u8; 32]) -> (u32, u32) {
    let key_a = (u32::from_le_bytes(digest[0..4].try_into().expect("4 bytes")) | 1) as u64;
    let key_b = (u32::from_le_bytes(digest[4..8].try_into().expect("4 bytes")) | 1) as u64;
    let factors = [
        key_a.wrapping_mul(CS64_FACTOR as u64) & U32_MASK,
        key_b.wrapping_mul(CS64_FACTOR as u64) & U32_MASK,
    ];
    let mut terms: Vec<u64> = CS64_COEFFICIENTS
        .iter()
        .enumerate()
        .map(|(index, (coefficient, _))| {
            ((*coefficient as u64) * factors[index % 2]) & U32_MASK
        })
        .collect();
    let mut offsets: Vec<u64> = CS64_COEFFICIENTS
        .iter()
        .map(|(_, offset)| *offset as u64)
        .collect();
    let mut seeded = vec![0u8];
    seeded.extend_from_slice(data);
    for (index, word) in words_le4_even(&seeded).into_iter().enumerate() {
        let w = word as u64;
        let mut term = w.wrapping_mul(if index % 2 == 0 { key_a } else { key_b });
        let mut offset = (rol16(word) as u64)
            .wrapping_mul(if index % 2 == 0 { 0xEF05_69FB } else { 0xC317_13DB });
        if index == 0 {
            term = term.wrapping_add(CS64_ADJUSTMENT0 as u64 * factors[0]);
            offset = offset.wrapping_add(CS64_ADJUSTMENT1 as u64);
        }
        terms.push(term & U32_MASK);
        offsets.push(offset & U32_MASK);
    }
    let (mut swap_acc, mut swap_last, mut state, mut acc) = (0u64, 0u64, 0u64, 0u64);
    for index in (0..terms.len()).step_by(2) {
        let x = terms[index];
        let y = terms[index + 1];
        let t = rol16(((offsets[index] + x) & U32_MASK) as u32) as u64;
        let q = (t.wrapping_mul(0x76F5_0F34)) & U32_MASK;
        swap_acc += (rol16(q as u32) as u64).wrapping_mul(0xC3EF_EA97) + q & U32_MASK;
        let t = rol16(((offsets[index + 1] + y) & U32_MASK) as u32) as u64;
        let q = (t.wrapping_mul(0x1881_CE3C)) & U32_MASK;
        swap_last = ((rol16(q as u32) as u64).wrapping_mul(0x35BD_1EC9) + q) & U32_MASK;
        swap_acc = (swap_acc + swap_last) & U32_MASK;

        state = (state * key_a + x) & U32_MASK;
        state = (rol16(state as u32) as u64).wrapping_mul(0xCF98_B111) & U32_MASK;
        state = (rol16(state as u32) as u64).wrapping_mul(0x8708_5B9F) & U32_MASK;
        state = (rol16(state as u32) as u64).wrapping_mul(0x12CE_B96D) & U32_MASK;
        state = (rol16(state as u32) as u64).wrapping_mul(0x257E_1D83) & U32_MASK;
        acc += state;
        let t = ((state * key_b + y) & U32_MASK) as u32;
        let t = (rol16(t) as u64).wrapping_mul(0xA274_16F5) & U32_MASK;
        let t = (rol16(t as u32) as u64).wrapping_mul(0xD383_96FF) & U32_MASK;
        state = (rol16((rol16(t as u32) as u64).wrapping_mul(0x7C93_2B89) as u32)
            as u64)
            .wrapping_mul(0xBFA4_9F69)
            & U32_MASK;
        acc = (acc + state) & U32_MASK;
    }
    ((swap_last ^ state) as u32, (swap_acc ^ acc) as u32)
}

/// 组合两个摘要（drm.py `_token_hash`）：cs64 小端 hex 在前、sha256 大端 hex 在后。
fn token_hash(data: &[u8]) -> (String, String) {
    let digest = sha256_digest(data);
    let (a, b) = cs64_digest(data, &digest);
    // pack("<II", a, b) 的 hex = 两 u32 的小端字节序列 hex，即各自 swap_bytes 后的大端打印
    let cs64 = format!("{:08X}{:08X}", a.swap_bytes(), b.swap_bytes());
    let sha256 = digest.iter().map(|byte| format!("{byte:02X}")).collect::<String>();
    (cs64, sha256)
}

/// 生成 v2 `Sec-MS-GEC`（drm.py `generate_sec_ms_gec_v2`）。
/// 时间窗与 v1 相同：Windows 文件时间向下取整到 5 分钟，再按 100ns 计。
/// 注意遵循 Python 实现的 f64 运算顺序（大数值下浮点取整参与哈希输入，不能改成整数运算）。
pub(crate) fn generate_sec_ms_gec_v2(unix_timestamp: f64) -> String {
    let mut ticks = unix_timestamp;
    ticks += WIN_EPOCH;
    ticks -= ticks % 300.0;
    ticks *= 1e7;
    let str_to_hash = format!("{ticks:.0}{TRUSTED_CLIENT_TOKEN}");
    let (cs64, sha256) = token_hash(str_to_hash.as_bytes());
    format!("{cs64}-{sha256}")
}

#[cfg(test)]
mod tests {
    use super::generate_sec_ms_gec_v2;

    /// Python 参考（rany2/edge-tts 7.3.0 drm.py，对固定时间戳生成）。
    const CASES: &[(f64, &str)] = &[
        (0.0, "52BC7A17B50DFEC9-28C4394775C8751CD2F6859C030C06DB1489ACAB3B3A7651831FC26895177ED6"),
        (1.0, "52BC7A17B50DFEC9-28C4394775C8751CD2F6859C030C06DB1489ACAB3B3A7651831FC26895177ED6"),
        (1_000_000_000.0, "5E9153719FADA922-14D2A75113EC5862E17303287DCDB197E3A671DF9FCC53C100D6B27928384B98"),
        (1_735_689_600.0, "54404AFE67C9B44A-E72E51D8B4F5F75E9A96B7E20D7877227455BC310433BD30C817CD54A1D9313E"),
        (1_792_962_470.0, "14A607087C9A9017-27080AB8A419FB3268E4DEFFF331EDE762553C88F1047CDB236AA79F93A6708D"),
        (4_102_444_800.0, "DD230DB44FEE2FD7-DA0D4FA797866E36C992FABB81DD2F9DB37EB74D675D63228277003DCD3F9F8C"),
    ];

    #[test]
    fn v2_matches_python_reference() {
        for (ts, expected) in CASES {
            let got = generate_sec_ms_gec_v2(*ts);
            assert_eq!(&got, expected, "ts={ts}");
        }
    }

    #[test]
    fn v2_格式为两段十六进制() {
        let token = generate_sec_ms_gec_v2(1_792_962_470.0);
        let (head, tail) = token.split_once('-').expect("应含分隔符 -");
        assert_eq!(head.len(), 16);
        assert_eq!(tail.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }
}
