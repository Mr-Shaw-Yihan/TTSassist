// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Static voice-table data for the offline fallback catalogue.
//!
//! Raw `(id, label)` tuples per language, plus a locale-keyed lookup. Held
//! separately from [`crate::voice_table`] so the data does not crowd the
//! normalization and formatting logic.

pub(crate) const VOICES_EN: &[(&str, &str)] = &[
    ("en-US-EmmaMultilingualNeural", "Emma (US)"),
    ("en-US-AndrewMultilingualNeural", "Andrew (US)"),
    ("en-US-AvaMultilingualNeural", "Ava (US)"),
    ("en-US-BrianNeural", "Brian (US)"),
    ("en-US-JennyNeural", "Jenny (US)"),
    ("en-US-GuyNeural", "Guy (US)"),
    ("en-GB-SoniaNeural", "Sonia (UK)"),
    ("en-GB-RyanNeural", "Ryan (UK)"),
    ("en-GB-LibbyNeural", "Libby (UK)"),
    ("en-AU-NatashaNeural", "Natasha (AU)"),
    ("en-AU-WilliamNeural", "William (AU)"),
    ("en-IN-NeerjaNeural", "Neerja (IN)"),
    ("en-CA-ClaraNeural", "Clara (CA)"),
    ("en-IE-EmilyNeural", "Emily (IE)"),
];

pub(crate) const VOICES_BN: &[(&str, &str)] = &[
    ("bn-BD-NabanitaNeural", "Nabanita (BD)"),
    ("bn-BD-PradeepNeural", "Pradeep (BD)"),
    ("bn-IN-TanishaaNeural", "Tanishaa (IN)"),
    ("bn-IN-BashkarNeural", "Bashkar (IN)"),
];

pub(crate) const VOICES_AR: &[(&str, &str)] = &[
    ("ar-SA-HamedNeural", "Hamed (SA)"),
    ("ar-SA-ZariyahNeural", "Zariyah (SA)"),
    ("ar-EG-SalmaNeural", "Salma (EG)"),
    ("ar-EG-ShakirNeural", "Shakir (EG)"),
];

pub(crate) const VOICES_HI: &[(&str, &str)] = &[
    ("hi-IN-SwaraNeural", "Swara (IN)"),
    ("hi-IN-MadhurNeural", "Madhur (IN)"),
];

pub(crate) const VOICES_JA: &[(&str, &str)] = &[
    ("ja-JP-NanamiNeural", "Nanami (JP)"),
    ("ja-JP-KeitaNeural", "Keita (JP)"),
];

pub(crate) const VOICES_TH: &[(&str, &str)] = &[
    ("th-TH-PremwadeeNeural", "Premwadee (TH)"),
    ("th-TH-NiwatNeural", "Niwat (TH)"),
];

pub(crate) const VOICES_EL: &[(&str, &str)] = &[
    ("el-GR-AthinaNeural", "Athina (GR)"),
    ("el-GR-NestorasNeural", "Nestoras (GR)"),
];

pub(crate) const VOICES_RU: &[(&str, &str)] = &[
    ("ru-RU-SvetlanaNeural", "Svetlana (RU)"),
    ("ru-RU-DmitryNeural", "Dmitry (RU)"),
];

pub(crate) const VOICES_HE: &[(&str, &str)] = &[
    ("he-IL-HilaNeural", "Hila (IL)"),
    ("he-IL-AvriNeural", "Avri (IL)"),
];

pub(crate) const VOICES_KA: &[(&str, &str)] = &[
    ("ka-GE-EkaNeural", "Eka (GE)"),
    ("ka-GE-GiorgiNeural", "Giorgi (GE)"),
];

pub(crate) const VOICES_AM: &[(&str, &str)] = &[
    ("am-ET-MekdesNeural", "Mekdes (ET)"),
    ("am-ET-AmehaNeural", "Ameha (ET)"),
];

pub(crate) const VOICES_GU: &[(&str, &str)] = &[
    ("gu-IN-DhwaniNeural", "Dhwani (IN)"),
    ("gu-IN-NiranjanNeural", "Niranjan (IN)"),
];

pub(crate) const VOICES_TA: &[(&str, &str)] = &[
    ("ta-IN-PallaviNeural", "Pallavi (IN)"),
    ("ta-IN-ValluvarNeural", "Valluvar (IN)"),
];

pub(crate) const VOICES_TE: &[(&str, &str)] = &[
    ("te-IN-ShrutiNeural", "Shruti (IN)"),
    ("te-IN-MohanNeural", "Mohan (IN)"),
];

pub(crate) const VOICES_KN: &[(&str, &str)] = &[
    ("kn-IN-SapnaNeural", "Sapna (IN)"),
    ("kn-IN-GaganNeural", "Gagan (IN)"),
];

pub(crate) const VOICES_ML: &[(&str, &str)] = &[
    ("ml-IN-SobhanaNeural", "Sobhana (IN)"),
    ("ml-IN-MidhunNeural", "Midhun (IN)"),
];

pub(crate) const VOICES_SI: &[(&str, &str)] = &[
    ("si-LK-ThiliniNeural", "Thilini (LK)"),
    ("si-LK-SameeraNeural", "Sameera (LK)"),
];

pub(crate) const VOICES_LO: &[(&str, &str)] = &[
    ("lo-LA-KeomanyNeural", "Keomany (LA)"),
    ("lo-LA-ChanthavongNeural", "Chanthavong (LA)"),
];

pub(crate) const VOICES_KM: &[(&str, &str)] = &[
    ("km-KH-SreymomNeural", "Sreymom (KH)"),
    ("km-KH-PisethNeural", "Piseth (KH)"),
];

pub(crate) const VOICES_MY: &[(&str, &str)] = &[
    ("my-MM-NilarNeural", "Nilar (MM)"),
    ("my-MM-ThihaNeural", "Thiha (MM)"),
];

pub(crate) const VOICES_KO: &[(&str, &str)] = &[
    ("ko-KR-SunHiNeural", "SunHi (KR)"),
    ("ko-KR-InJoonNeural", "InJoon (KR)"),
];

pub(crate) const VOICES_ZH: &[(&str, &str)] = &[
    ("zh-CN-XiaoxiaoNeural", "Xiaoxiao (CN)"),
    ("zh-CN-YunyangNeural", "Yunyang (CN)"),
];

// Armenian and Punjabi (Gurmukhi) have no dedicated Edge voice. The
// multilingual voice is assigned as a placeholder, but it does not officially
// support these scripts - output may be English-accented or fail to synth.
// If a dedicated voice ships, replace these entries.
pub(crate) const VOICES_HY: &[(&str, &str)] = &[("en-US-EmmaMultilingualNeural", "Emma (Multi)")];
pub(crate) const VOICES_PA: &[(&str, &str)] = &[("en-US-EmmaMultilingualNeural", "Emma (Multi)")];

pub(crate) const ALL_FALLBACK: &[&[(&str, &str)]] = &[
    VOICES_EN, VOICES_BN, VOICES_AR, VOICES_HI, VOICES_JA, VOICES_TH, VOICES_EL, VOICES_RU,
    VOICES_HE, VOICES_KA, VOICES_AM, VOICES_GU, VOICES_TA, VOICES_TE, VOICES_KN, VOICES_ML,
    VOICES_SI, VOICES_LO, VOICES_KM, VOICES_MY, VOICES_KO, VOICES_ZH, VOICES_HY, VOICES_PA,
];

/// Resolve a normalized locale to its fallback `(id, label)` table. Unknown
/// locales fall back to the English table.
pub(crate) fn fallback_table(normalized: &str) -> &'static [(&'static str, &'static str)] {
    match normalized {
        "bn-BD" => VOICES_BN,
        "ar-SA" => VOICES_AR,
        "hi-IN" => VOICES_HI,
        "ja-JP" => VOICES_JA,
        "th-TH" => VOICES_TH,
        "el-GR" => VOICES_EL,
        "ru-RU" => VOICES_RU,
        "he-IL" => VOICES_HE,
        "ka-GE" => VOICES_KA,
        "hy-AM" => VOICES_HY,
        "am-ET" => VOICES_AM,
        "gu-IN" => VOICES_GU,
        "pa-IN" => VOICES_PA,
        "ta-IN" => VOICES_TA,
        "te-IN" => VOICES_TE,
        "kn-IN" => VOICES_KN,
        "ml-IN" => VOICES_ML,
        "si-LK" => VOICES_SI,
        "lo-LA" => VOICES_LO,
        "km-KH" => VOICES_KM,
        "my-MM" => VOICES_MY,
        "ko-KR" => VOICES_KO,
        "zh-CN" => VOICES_ZH,
        _ => VOICES_EN,
    }
}
