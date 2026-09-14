// 内置效果器包（builtin-fx）：随宿主内置分发的 audio_effect 插件。
//
// v1 预设 = 「原声 off」保留值（不在此插件内）+ 本插件效果：
//   electric   电音（环形调制，免变调）
//   distorted  失真电子声（削波，附赠）
//   telephone  电话音（带通+降采样，附赠）
//   chipmunk   花栗鼠（音高上移，需变调；granular 单独提交）
//   bass_boost 低音炮（音高下移+低通，需变调）
// 对标短视频剪辑工具常见人声效果的**听感**（仅听感参照，不含任何其素材/参数，
// 见设计 §八 UI-5）。effect_id 全 ASCII 蛇形；显示名人类可读中文。

mod dsp;

use plugin_api::{FxProcessInput, FxProcessOutput};

plugin_api::va_fx_plugin! {
    id: "builtin-fx",
    name: "内置效果器包",
    version: "1.0.0",
    effects_json: r#"[
        {"id":"electric","label":"电音","description":"金属机械感的环形调制人声","params":[
            {"key":"carrier","label":"载波频率","min":40,"max":120,"default":65,"unit":"Hz"},
            {"key":"mix","label":"调制深度","min":0,"max":1,"default":0.65,"unit":""}
        ]},
        {"id":"chipmunk","label":"花栗鼠","description":"音高上移的欢快细声","params":[
            {"key":"semitones","label":"升调半音","min":1,"max":8,"default":4,"unit":"st"}
        ]},
        {"id":"bass_boost","label":"低音炮","description":"音高下移加低通的浑厚恶魔声","params":[
            {"key":"semitones","label":"降调半音","min":-8,"max":-1,"default":-4,"unit":"st"},
            {"key":"lowpass","label":"低通截止","min":800,"max":4000,"default":2000,"unit":"Hz"}
        ]},
        {"id":"distorted","label":"失真电子声","description":"削波失真的电子嗓音","params":[
            {"key":"drive","label":"失真强度","min":1.5,"max":8,"default":5,"unit":""}
        ]},
        {"id":"telephone","label":"电话音","description":"带通加降采样的对讲机听感","params":[]}
    ]"#,
    process: process,
}

fn process(input: &FxProcessInput) -> Result<FxProcessOutput, String> {
    let mut pcm = input.pcm.to_vec();
    match input.effect_id {
        "electric" => {
            let carrier = dsp::param_f64(input.params_json, "carrier", 65.0) as f32;
            let mix = dsp::param_f64(input.params_json, "mix", 0.65) as f32;
            dsp::electric(&mut pcm, input.channels, input.sample_rate, carrier, mix.clamp(0.0, 1.0));
        }
        "distorted" => {
            let drive = dsp::param_f64(input.params_json, "drive", 5.0) as f32;
            dsp::distorted(&mut pcm, input.channels, input.sample_rate, drive.max(1.0));
        }
        "telephone" => {
            dsp::telephone(&mut pcm, input.channels, input.sample_rate);
        }
        "chipmunk" => {
            let semis = dsp::param_f64(input.params_json, "semitones", 4.0) as f32;
            dsp::pitch_shift(&mut pcm, input.channels, input.sample_rate, semis);
            dsp::normalize(&mut pcm, 0.9);
        }
        "bass_boost" => {
            let semis = dsp::param_f64(input.params_json, "semitones", -4.0) as f32;
            let lowpass = dsp::param_f64(input.params_json, "lowpass", 2_000.0) as f32;
            dsp::pitch_shift(&mut pcm, input.channels, input.sample_rate, semis);
            let mut lp = dsp::Biquad::lowpass(input.sample_rate, lowpass, 0.707);
            for c in 0..input.channels as usize {
                lp.process_channel(&mut pcm, input.channels, c);
            }
            dsp::normalize(&mut pcm, 0.9);
        }
        other => return Err(format!("未知效果「{other}」")),
    }
    Ok(FxProcessOutput {
        pcm,
        frames: input.frames,
        channels: input.channels,
    })
}
