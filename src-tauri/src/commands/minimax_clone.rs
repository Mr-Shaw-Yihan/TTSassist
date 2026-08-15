// MiniMax 国际版音色克隆与音色管理命令。
//
// 仅服务 minimax-tts-global 插件（国内版为阉割版，不提供克隆）。
// 端点（base_url = https://api.minimax.io）：
//   POST /v1/files/upload   multipart（purpose + file）→ file_id
//   POST /v1/voice_clone    {file_id, voice_id, clone_prompt?}
//   POST /v1/get_voice      {voice_type}
//   POST /v1/delete_voice   {voice_type, voice_id}
//
// 注意：克隆音色须成功合成过一次后才会出现在 get_voice 列表；
//       delete_voice 删除后 voice_id 不可复用。

use serde::Deserialize;

/// 文件上传响应
#[derive(Deserialize)]
struct FileUploadResp {
    file: Option<FileData>,
    base_resp: Option<BaseResp>,
}

#[derive(Deserialize)]
struct FileData {
    file_id: i64,
}

#[derive(Deserialize)]
struct BaseResp {
    status_code: i64,
    status_msg: Option<String>,
}

/// 音色管理查询响应（原样透传 JSON 给前端解析）
#[derive(Deserialize)]
struct GetVoiceResp {
    #[allow(dead_code)]
    system_voice: Option<Vec<serde_json::Value>>,
    #[allow(dead_code)]
    voice_cloning: Option<Vec<serde_json::Value>>,
    #[allow(dead_code)]
    voice_generation: Option<Vec<serde_json::Value>>,
    base_resp: Option<BaseResp>,
}

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("HTTP 客户端创建失败: {e}"))
}

fn check_base_resp(br: &Option<BaseResp>, ctx: &str) -> Result<(), String> {
    if let Some(br) = br {
        if br.status_code != 0 {
            let msg = br.status_msg.as_deref().unwrap_or("未知错误");
            // 2038：无克隆权限 → 引导用户检查账号认证状态
            if br.status_code == 2038 {
                return Err(format!(
                    "{ctx} {}: {msg}（无克隆权限，请到 MiniMax 平台检查账号认证状态）",
                    br.status_code
                ));
            }
            return Err(format!("{ctx} {}: {msg}", br.status_code));
        }
    }
    Ok(())
}

/// 上传音频文件（multipart，purpose 区分用途），返回 file_id。
async fn upload_file(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    file_path: &str,
    purpose: &str,
) -> Result<i64, String> {
    let file_bytes = std::fs::read(file_path)
        .map_err(|e| format!("读取音频文件失败: {e}"))?;

    if file_bytes.len() > 20 * 1024 * 1024 {
        return Err("音频文件超过 20MB 限制".into());
    }

    let file_name = std::path::Path::new(file_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "audio.mp3".to_string());

    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("application/octet-stream")
        .map_err(|e| format!("构造上传请求失败: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .text("purpose", purpose.to_string())
        .part("file", part);

    let resp: FileUploadResp = client
        .post(format!("{}/v1/files/upload", base_url))
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("文件上传请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("文件上传响应解析失败: {e}"))?;

    check_base_resp(&resp.base_resp, "文件上传失败")?;

    resp.file
        .map(|f| f.file_id)
        .ok_or_else(|| "文件上传响应中缺少 file_id".to_string())
}

/// 克隆 MiniMax 国际版音色。
///
/// 1. 上传主音频（purpose=voice_clone，10s~5min，mp3/m4a/wav，≤20MB）
/// 2. 可选上传样本音频（purpose=prompt_audio，<8s）+ 对应文本，提升相似度
/// 3. 调 /v1/voice_clone，返回克隆的 voice_id
#[tauri::command]
pub async fn minimax_global_voice_clone(
    file_path: String,
    voice_id: String,
    api_key: String,
    base_url: String,
    prompt_file_path: Option<String>,
    prompt_text: Option<String>,
) -> Result<String, String> {
    // 校验 voice_id 命名规则（官方：8~256，字母开头，字母/数字/-/_，末位非 -/_）
    if voice_id.len() < 8 || voice_id.len() > 256 {
        return Err("voice_id 长度须在 8~256 字符之间".into());
    }
    let first = voice_id.chars().next().unwrap_or(' ');
    if !first.is_ascii_alphabetic() {
        return Err("voice_id 首字符必须为英文字母".into());
    }
    if voice_id.ends_with('-') || voice_id.ends_with('_') {
        return Err("voice_id 末位不能是 - 或 _".into());
    }
    if !voice_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("voice_id 只允许字母、数字、-、_".into());
    }

    let client = build_client()?;

    // 1. 上传主音频
    let file_id = upload_file(&client, &base_url, &api_key, &file_path, "voice_clone").await?;

    // 2. 可选：上传样本音频（clone_prompt 两字段须同时提供）
    let clone_prompt = match (&prompt_file_path, &prompt_text) {
        (Some(p), Some(t)) if !p.is_empty() && !t.trim().is_empty() => {
            let prompt_id =
                upload_file(&client, &base_url, &api_key, p, "prompt_audio").await?;
            Some(serde_json::json!({
                "prompt_audio": prompt_id,
                "prompt_text": t.trim(),
            }))
        }
        _ => None,
    };

    // 3. 调用 voice_clone
    let mut body = serde_json::json!({
        "file_id": file_id,
        "voice_id": voice_id,
    });
    if let Some(cp) = clone_prompt {
        body["clone_prompt"] = cp;
    }

    let resp = client
        .post(format!("{}/v1/voice_clone", base_url))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("音色克隆请求失败: {e}"))?;

    #[derive(Deserialize)]
    struct CloneResp {
        base_resp: Option<BaseResp>,
    }
    let clone_resp: CloneResp = resp
        .json()
        .await
        .map_err(|e| format!("音色克隆响应解析失败: {e}"))?;

    check_base_resp(&clone_resp.base_resp, "音色克隆失败")?;

    Ok(voice_id)
}

/// 查询账号音色列表（system / voice_cloning / voice_generation），原样返回 JSON。
#[tauri::command]
pub async fn minimax_global_get_voices(
    api_key: String,
    base_url: String,
) -> Result<String, String> {
    let client = build_client()?;

    let resp = client
        .post(format!("{}/v1/get_voice", base_url))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({ "voice_type": "all" }))
        .send()
        .await
        .map_err(|e| format!("音色查询请求失败: {e}"))?;

    let text = resp
        .text()
        .await
        .map_err(|e| format!("音色查询响应读取失败: {e}"))?;

    // 先反序列化检查业务错误，再透传原始 JSON 给前端
    let parsed: GetVoiceResp = serde_json::from_str(&text)
        .map_err(|e| format!("音色查询响应解析失败: {e}"))?;
    check_base_resp(&parsed.base_resp, "音色查询失败")?;

    Ok(text)
}

/// 删除克隆/设计音色（删除后 voice_id 不可复用）。
#[tauri::command]
pub async fn minimax_global_delete_voice(
    api_key: String,
    base_url: String,
    voice_type: String,
    voice_id: String,
) -> Result<String, String> {
    if voice_type != "voice_cloning" && voice_type != "voice_generation" {
        return Err("仅支持删除克隆音色或设计音色".into());
    }

    let client = build_client()?;

    #[derive(Deserialize)]
    struct DeleteResp {
        base_resp: Option<BaseResp>,
    }

    let resp: DeleteResp = client
        .post(format!("{}/v1/delete_voice", base_url))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "voice_type": voice_type,
            "voice_id": voice_id,
        }))
        .send()
        .await
        .map_err(|e| format!("音色删除请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("音色删除响应解析失败: {e}"))?;

    check_base_resp(&resp.base_resp, "音色删除失败")?;

    Ok(voice_id)
}
