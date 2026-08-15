// MiniMax TTS API 共享核心库。
//
// 封装 MiniMax Text-to-Audio V2 HTTP API 的调用逻辑，供国内版（minimax-tts）
// 和国际版（minimax-tts-global）两个插件 crate 复用。
//
// 国内版端点：https://api.minimaxi.com/v1/t2a_v2
// 国际版端点：https://api.minimax.io/v1/t2a_v2

use serde::{Deserialize, Serialize};

// 重新导出 plugin_api::VoiceItem 供插件 crate 使用
pub use plugin_api::VoiceItem;

/// MiniMax TTS API 默认模型
pub const DEFAULT_MODEL: &str = "speech-2.8-hd";

/// 默认音色（甜美女性）
pub const DEFAULT_VOICE_ID: &str = "female-tianmei";

// ── API 请求/响应结构 ──────────────────────────────────────

#[derive(Serialize)]
struct T2aRequest {
    model: String,
    text: String,
    stream: bool,
    voice_setting: VoiceSetting,
    audio_setting: AudioSetting,
}

#[derive(Serialize)]
struct VoiceSetting {
    voice_id: String,
    speed: f64,
    vol: f64,
    pitch: i32,
}

#[derive(Serialize)]
struct AudioSetting {
    sample_rate: u32,
    bitrate: u32,
    format: String,
    channel: u32,
}

#[derive(Deserialize)]
struct T2aResponse {
    data: Option<ResponseData>,
    base_resp: Option<BaseResp>,
}

#[derive(Deserialize)]
struct ResponseData {
    audio: Option<String>,
    #[allow(dead_code)]
    status: Option<i32>,
}

#[derive(Deserialize)]
struct BaseResp {
    status_code: i32,
    status_msg: Option<String>,
}

// ── 核心合成函数 ──────────────────────────────────────────

/// 调用 MiniMax T2A V2 API 合成语音，返回 MP3 音频字节。
///
/// - `base_url`：API 端点前缀，如 `https://api.minimaxi.com` 或 `https://api.minimax.io`
/// - `api_key_env`：环境变量名，如 `MINIMAX_API_KEY` 或 `MINIMAX_GLOBAL_API_KEY`
/// - `text`：待合成文本
/// - `voice_id`：音色 ID（为 None 时使用默认音色）
pub fn synthesize(
    base_url: &str,
    api_key_env: &str,
    text: &str,
    voice_id: Option<&str>,
) -> Result<Vec<u8>, String> {
    let api_key = read_api_key(api_key_env)?;
    let url = format!("{}/v1/t2a_v2", base_url);

    let req = T2aRequest {
        model: DEFAULT_MODEL.to_string(),
        text: text.to_string(),
        stream: false,
        voice_setting: VoiceSetting {
            voice_id: voice_id.unwrap_or(DEFAULT_VOICE_ID).to_string(),
            speed: 1.0,
            vol: 1.0,
            pitch: 0,
        },
        audio_setting: AudioSetting {
            sample_rate: 32000,
            bitrate: 128000,
            format: "mp3".to_string(),
            channel: 1,
        },
    };

    let resp: T2aResponse = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", api_key))
        .send_json(&req)
        .map_err(|e| format!("MiniMax TTS 请求失败: {e}"))?
        .into_json()
        .map_err(|e| format!("MiniMax TTS 响应解析失败: {e}"))?;

    // 检查业务错误
    if let Some(br) = &resp.base_resp {
        if br.status_code != 0 {
            return Err(format!(
                "MiniMax TTS 错误 {}: {}",
                br.status_code,
                br.status_msg.as_deref().unwrap_or("未知错误")
            ));
        }
    }

    // 提取 hex 编码的音频
    let hex_audio = resp
        .data
        .and_then(|d| d.audio)
        .ok_or("MiniMax TTS 未返回音频数据")?;

    if hex_audio.is_empty() {
        return Err("MiniMax TTS 返回空音频".to_string());
    }

    hex_decode(&hex_audio).map_err(|e| format!("MiniMax TTS 音频 hex 解码失败: {e}"))
}

// ── Hex 解码 ──────────────────────────────────────────────

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex 字符串长度为奇数".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| format!("hex 解码错误 @{}: {}", i, e))
        })
        .collect()
}

// ── API Key 读取 ──────────────────────────────────────────

fn read_api_key(env_var: &str) -> Result<String, String> {
    // 先查指定环境变量，再查通用 MINIMAX_API_KEY 作为后备
    std::env::var(env_var)
        .or_else(|_| std::env::var("MINIMAX_API_KEY"))
        .map_err(|_| {
            format!(
                "未设置 MiniMax API Key，请设置环境变量 {} 或 MINIMAX_API_KEY。\n\
                 获取地址：https://platform.minimaxi.com/user-center/basic-information/interface-key",
                env_var
            )
        })
}

// ── 音色列表 ──────────────────────────────────────────────

/// 音色列表 JSON（静态字符串，供 voices_json 宏变体或运行时解析使用）。
///
/// 精选 50 个常用音色，覆盖中文、英文、日文、韩文等主要语种。
/// 用户也可手动输入不在此列表中的 MiniMax 系统音色 ID。
pub fn voices_json() -> &'static str {
    VOICES_JSON
}

/// 动态音色列表函数（供 va_tts_plugin! 的 `voices:` 变体使用）。
/// 每次调用解析 JSON 返回 Vec<VoiceItem>，宿主立即拷贝不长期持有。
pub fn voices_list() -> Vec<VoiceItem> {
    serde_json::from_str(VOICES_JSON).unwrap_or_default()
}

const VOICES_JSON: &str = r#"[
{"id":"female-tianmei","label":"甜美女性（中文）"},
{"id":"female-shaonv","label":"少女（中文）"},
{"id":"female-yujie","label":"御姐（中文）"},
{"id":"female-chengshu","label":"成熟女性（中文）"},
{"id":"male-qn-qingse","label":"青涩青年（中文）"},
{"id":"male-qn-jingying","label":"精英青年（中文）"},
{"id":"male-qn-badao","label":"霸道青年（中文）"},
{"id":"male-qn-daxuesheng","label":"大学生（中文）"},
{"id":"Chinese (Mandarin)_Sweet_Lady","label":"甜美女声"},
{"id":"Chinese (Mandarin)_Gentleman","label":"温润男声"},
{"id":"Chinese (Mandarin)_Warm_Bestie","label":"温暖闺蜜"},
{"id":"Chinese (Mandarin)_Radio_Host","label":"电台男主播"},
{"id":"Chinese (Mandarin)_Lyrical_Voice","label":"抒情男声"},
{"id":"Chinese (Mandarin)_News_Anchor","label":"新闻女声"},
{"id":"Chinese (Mandarin)_Male_Announcer","label":"播报男声"},
{"id":"Chinese (Mandarin)_Soft_Girl","label":"柔和少女"},
{"id":"Chinese (Mandarin)_Wise_Women","label":"阅历姐姐"},
{"id":"Chinese (Mandarin)_Southern_Young_Man","label":"南方小哥"},
{"id":"lovely_girl","label":"萌萌女童"},
{"id":"clever_boy","label":"聪明男童"},
{"id":"Cantonese_GentleLady","label":"粤语·温柔女声"},
{"id":"Cantonese_PlayfulMan","label":"粤语·活泼男声"},
{"id":"English_Graceful_Lady","label":"英文·Graceful Lady"},
{"id":"English_Trustworthy_Man","label":"英文·Trustworthy Man"},
{"id":"English_Insightful_Speaker","label":"英文·Insightful Speaker"},
{"id":"English_radiant_girl","label":"英文·Radiant Girl"},
{"id":"English_Persuasive_Man","label":"英文·Persuasive Man"},
{"id":"Attractive_Girl","label":"英文·Attractive Girl"},
{"id":"Serene_Woman","label":"英文·Serene Woman"},
{"id":"Sweet_Girl","label":"英文·Sweet Girl"},
{"id":"Japanese_GracefulMaiden","label":"日文·Graceful Maiden"},
{"id":"Japanese_GentleButler","label":"日文·Gentle Butler"},
{"id":"Japanese_KindLady","label":"日文·Kind Lady"},
{"id":"Japanese_DominantMan","label":"日文·Dominant Man"},
{"id":"Japanese_ColdQueen","label":"日文·Cold Queen"},
{"id":"Korean_SweetGirl","label":"韩文·Sweet Girl"},
{"id":"Korean_CheerfulBoyfriend","label":"韩文·Cheerful Boyfriend"},
{"id":"Korean_CalmLady","label":"韩文·Calm Lady"},
{"id":"Korean_DominantMan","label":"韩文·Dominant Man"},
{"id":"Spanish_SereneWoman","label":"西语·Serene Woman"},
{"id":"Spanish_Narrator","label":"西语·Narrator"},
{"id":"French_Male_Speech_New","label":"法语·Level-Headed Man"},
{"id":"French_FemaleAnchor","label":"法语·Female Anchor"},
{"id":"German_FriendlyMan","label":"德语·Friendly Man"},
{"id":"German_SweetLady","label":"德语·Sweet Lady"},
{"id":"Russian_ReliableMan","label":"俄语·Reliable Man"},
{"id":"Russian_AmbitiousWoman","label":"俄语·Ambitious Woman"},
{"id":"Indonesian_CalmWoman","label":"印尼·Calm Woman"},
{"id":"Arabic_CalmWoman","label":"阿语·Calm Woman"},
{"id":"Thai_female_1_sample1","label":"泰语·Confident Woman"}
]"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_decode_正常() {
        assert_eq!(hex_decode("48656c6c6f").unwrap(), b"Hello");
        assert_eq!(hex_decode("").unwrap(), b"");
        assert_eq!(hex_decode("00ff").unwrap(), vec![0, 255]);
    }

    #[test]
    fn hex_decode_奇数长度报错() {
        assert!(hex_decode("abc").is_err());
    }

    #[test]
    fn 音色列表是合法json() {
        let v: Vec<serde_json::Value> = serde_json::from_str(voices_json()).unwrap();
        assert!(v.len() >= 40);
        // 每条记录都有 id 和 label
        assert!(v[0].get("id").is_some());
        assert!(v[0].get("label").is_some());
    }
}
