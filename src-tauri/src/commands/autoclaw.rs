//! AutoClaw（智谱「澳龙」）命令域：账号池 / 签到 / 积分 / 环境配置 / 计划任务。
//!
//! 与其它应用域一致的红线：
//! - 全部 serde 字段 snake_case，与前端 `types.ts` 逐字对齐；
//! - accessToken 只进 `autoclaw_tokens` 文档，视图仅回 `has_credential` + uid 尾号；
//! - 签到进度走 NDJSON 字符串事件（`autoclaw-checkin-progress`），前端逐行 JSON.parse。

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::fs_utils;
use crate::state::AppState;
use crate::tasks::autoclaw as core;

/// schtasks 计划任务名（CLI 任务模式 `--task-run autoclaw-checkin`）
pub const AUTOCLAW_TASK_NAME: &str = "AIWorkAssistant_AutoClawCheckin";

/// 单轮签到互斥（UI 连点 + CLI 计划任务并发时幂等跳过）
static ROUND_BUSY: AtomicBool = AtomicBool::new(false);

struct RoundGuard;

impl Drop for RoundGuard {
    fn drop(&mut self) {
        ROUND_BUSY.store(false, Ordering::SeqCst);
    }
}

fn try_acquire_round() -> Option<RoundGuard> {
    ROUND_BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| RoundGuard)
}

// ── 环境检测 ──────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct AutoClawEnvCheck {
    pub installed: bool,
    pub running: bool,
    pub exe_path: Option<String>,
    pub data_dir: Option<String>,
    pub accounts_dir: Option<String>,
    pub has_local_state: bool,
    pub account_dirs: usize,
}

fn app_path_of(state: &AppState) -> Option<String> {
    let s = core::load_settings(state);
    if !s.app_path.trim().is_empty() && std::path::Path::new(s.app_path.trim()).is_file() {
        return Some(s.app_path.trim().to_string());
    }
    if let Some(p) = crate::switcher::proc::running_exe_exact(&["AutoClaw", "AutoClaw2", "AutoClaw-official"]) {
        return Some(p.to_string_lossy().to_string());
    }
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    for key in ["LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)"] {
        if let Ok(base) = std::env::var(key) {
            let base = std::path::PathBuf::from(base);
            for (dir, exe) in [
                ("AutoClaw", "AutoClaw.exe"),
                ("AutoClaw2", "AutoClaw2.exe"),
                ("Programs\\AutoClaw", "AutoClaw.exe"),
                ("Programs\\AutoClaw2", "AutoClaw2.exe"),
            ] {
                cands.push(base.join(dir).join(exe));
            }
        }
    }
    cands.into_iter().find(|p| p.is_file()).map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn autoclaw_env_check(state: State<AppState>) -> AutoClawEnvCheck {
    let data_dir = core::data_dir_of(&state);
    let accounts_dir = data_dir.as_ref().map(|d| d.join("accounts"));
    let account_dirs = accounts_dir
        .as_ref()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|it| it.flatten().filter(|e| e.path().is_dir()).count())
        .unwrap_or(0);
    let exe = app_path_of(&state);
    AutoClawEnvCheck {
        installed: exe.is_some(),
        running: crate::switcher::proc::any_running(&["AutoClaw", "AutoClaw2", "AutoClaw-official"]),
        exe_path: exe,
        has_local_state: data_dir.as_ref().map(|d| d.join("Local State").is_file()).unwrap_or(false),
        accounts_dir: accounts_dir.map(|d| d.to_string_lossy().to_string()),
        data_dir: data_dir.map(|d| d.to_string_lossy().to_string()),
        account_dirs,
    }
}

// ── 设置 ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn autoclaw_settings_get(state: State<AppState>) -> core::AutoClawSettings {
    core::load_settings(&state)
}

#[tauri::command]
pub fn autoclaw_settings_set(
    state: State<AppState>,
    patch: core::AutoClawSettings,
) -> Result<(), String> {
    if patch.auto_checkin {
        crate::commands::misc::validate_hhmm(&patch.checkin_hhmm)?;
    }
    core::save_settings(&state, &patch)
}

// ── 账号池 ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn autoclaw_accounts_list(state: State<AppState>) -> Vec<core::AccountView> {
    core::load_pool(&state)
        .iter()
        .map(|a| core::view_of(a, core::token_of(&state, &a.id).is_some()))
        .collect()
}

fn upsert_account(
    state: &AppState,
    uid: &str,
    nickname: &str,
    region: &str,
    source: &str,
    token: &str,
    refresh: &str,
) -> Result<core::AutoClawAccount, String> {
    let uid = uid.trim().to_string();
    if uid.is_empty() {
        return Err("账号 uid 为空，无法入池".into());
    }
    let id = core::account_id_of(&uid);
    let region = if region.eq_ignore_ascii_case("global") { "global" } else { "cn" }.to_string();
    let nick = if nickname.trim().is_empty() {
        let tail: String = uid.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
        format!("AutoClaw {tail}")
    } else {
        nickname.trim().to_string()
    };
    let expires = core::jwt_claims(token).get("exp").and_then(Value::as_i64);
    let mut saved = core::AutoClawAccount::default();
    core::with_pool_mut(state, |accounts| {
        if let Some(a) = accounts.iter_mut().find(|a| a.uid == uid || a.id == id) {
            a.uid = uid.clone();
            a.nickname = nick.clone();
            a.region = region.clone();
            a.credential_source = source.to_string();
            a.needs_relogin = false;
            a.relogin_reason.clear();
            a.token_expires_at = expires;
            saved = a.clone();
        } else {
            let acc = core::AutoClawAccount {
                id: id.clone(),
                uid: uid.clone(),
                nickname: nick.clone(),
                region: region.clone(),
                credential_source: source.to_string(),
                token_expires_at: expires,
                ..Default::default()
            };
            accounts.push(acc.clone());
            saved = acc;
        }
        Ok(())
    })?;
    core::set_token(
        state,
        &saved.id,
        &core::TokenEntry {
            access_token: token.trim().to_string(),
            refresh_token: refresh.trim().to_string(),
        },
    )?;
    Ok(saved)
}

/// 手工粘贴 accessToken 导入（官方无 PAT 体系，token 来自本机登录态或抓包）
#[tauri::command(async)]
pub fn autoclaw_import_token(
    state: State<AppState>,
    token: String,
    region: Option<String>,
    name: Option<String>,
) -> Result<core::AccountView, String> {
    let token = token.trim().to_string();
    if token.len() < 20 {
        return Err("accessToken 过短，请确认复制完整".into());
    }
    let region = region.unwrap_or_else(|| "cn".into());
    // 轻量校验：任务列表接口（只读、无副作用）→ 401/410000 直接拒绝
    let url = format!("{}/autoclaw-proxy/proxy/autoclaw-task-list?lang=zh-CN", core::base_of(&region));
    match core::http_json("GET", &url, &token, None) {
        Ok((status, v)) => {
            let code = v.get("code").and_then(Value::as_i64);
            if status == 401 || code == Some(410000) {
                return Err("token 无效或已过期（401/410000），请重新登录 AutoClaw 后复制".into());
            }
        }
        Err(e) => return Err(format!("校验 token 失败：{e}")),
    }
    let uid = core::uid_of_token(&token);
    let jwt_name = core::nickname_of_token(&token);
    let nick = name.or_else(|| if jwt_name.is_empty() { None } else { Some(jwt_name) }).unwrap_or_default();
    let acc = upsert_account(&state, &uid, &nick, &region, "token", &token, "")?;
    Ok(core::view_of(&acc, true))
}

#[derive(Serialize)]
pub struct ScanResult {
    pub data_dir: Option<String>,
    pub found: usize,
    pub imported: usize,
    pub updated: usize,
    pub errors: Vec<String>,
}

/// 扫描本机 AutoClaw 登录态（accounts/*/account-credentials.enc，DPAPI+AES-GCM）
#[tauri::command(async)]
pub fn autoclaw_scan_local(state: State<AppState>) -> ScanResult {
    let Some(dir) = core::data_dir_of(&state) else {
        return ScanResult {
            data_dir: None,
            found: 0,
            imported: 0,
            updated: 0,
            errors: vec!["未找到 AutoClaw 数据目录（可在环境配置页手动指定）".into()],
        };
    };
    let hits = core::scan_local(&dir);
    let mut imported = 0usize;
    let mut updated = 0usize;
    let mut errors: Vec<String> = Vec::new();
    if hits.is_empty() {
        errors.push("数据目录存在，但未发现可解密的账号凭证（若曾在 AutoClaw 登录过，请先用当前 Windows 用户打开一次客户端）".into());
    }
    for h in hits {
        let existed = core::load_pool(&state).iter().any(|a| a.uid == h.uid);
        match upsert_account(&state, &h.uid, &h.nickname, &h.region, "local_scan", &h.access_token, &h.refresh_token) {
            Ok(_) => {
                if existed {
                    updated += 1;
                } else {
                    imported += 1;
                }
            }
            Err(e) => errors.push(format!("{}: {e}", h.source_path)),
        }
    }
    fs_utils::app_log(
        &state.data_dir,
        &format!("AutoClaw 本机账号扫描：新增 {imported} / 更新 {updated}（{}）", dir.to_string_lossy()),
    );
    ScanResult { data_dir: Some(dir.to_string_lossy().to_string()), found: imported + updated, imported, updated, errors }
}

#[tauri::command]
pub fn autoclaw_account_update(
    state: State<AppState>,
    id: String,
    name: Option<String>,
    note: Option<String>,
    region: Option<String>,
) -> Result<(), String> {
    core::with_pool_mut(&state, |accounts| {
        let a = accounts.iter_mut().find(|a| a.id == id).ok_or("账号不存在")?;
        if let Some(n) = name {
            if !n.trim().is_empty() {
                a.nickname = n.trim().to_string();
            }
        }
        if let Some(n) = note {
            a.note = n;
        }
        if let Some(r) = region {
            a.region = if r.eq_ignore_ascii_case("global") { "global" } else { "cn" }.to_string();
        }
        Ok(())
    })
}

#[tauri::command]
pub fn autoclaw_account_remove(state: State<AppState>, id: String) -> Result<(), String> {
    core::with_pool_mut(&state, |accounts| {
        let before = accounts.len();
        accounts.retain(|a| a.id != id);
        if accounts.len() == before {
            return Err("账号不存在".into());
        }
        Ok(())
    })?;
    core::remove_token(&state, &id)
}

// ── 签到 ──────────────────────────────────────────────────────────────────

fn emit_line(app: &AppHandle, ev: &Value) {
    if let Ok(line) = serde_json::to_string(ev) {
        let _ = app.emit("autoclaw-checkin-progress", &line);
    }
}

#[tauri::command(async)]
pub fn autoclaw_checkin_start(
    app: AppHandle,
    state: State<AppState>,
    user_ids: Option<Vec<String>>,
    regions: Option<Vec<String>>,
) -> Result<(), String> {
    let guard = try_acquire_round().ok_or("已有 AutoClaw 签到任务在执行中，请稍后再试")?;
    let uids = user_ids.unwrap_or_default();
    let regs = regions.unwrap_or_default();
    let app2 = app.clone();
    let state2 = state.inner().clone();
    let spawned = std::thread::Builder::new().name("autoclaw-checkin".into()).spawn(move || {
        let _guard = guard;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            core::run_round(&state2, &uids, &regs, &mut |ev| emit_line(&app2, ev))
        }));
        let ok = result.is_ok();
        if !ok {
            fs_utils::app_log(&state2.data_dir, "AutoClaw 签到线程 panic（已捕获）");
        }
        let line = json!({ "type": "exit", "ok": ok }).to_string();
        let _ = app2.emit("autoclaw-checkin-progress", &line);
    });
    if let Err(e) = spawned {
        return Err(format!("签到线程启动失败：{e}"));
    }
    Ok(())
}

#[tauri::command]
pub fn autoclaw_checkin_results(
    state: State<AppState>,
    days: Option<i64>,
) -> Vec<core::CheckinRecord> {
    core::load_results(&state, days.unwrap_or(30))
}

// ── 积分 ──────────────────────────────────────────────────────────────────

#[tauri::command(async)]
pub fn autoclaw_credits_fetch(
    state: State<AppState>,
    user_id: Option<String>,
    fresh: Option<bool>,
) -> Value {
    core::credits_fetch(&state, user_id.as_deref(), fresh.unwrap_or(false))
}

#[tauri::command]
pub fn autoclaw_credits_history(state: State<AppState>) -> Vec<Value> {
    core::credits_history(&state)
}

// ── 概览 ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct Dashboard {
    pub accounts: usize,
    pub checked_today: usize,
    pub total_credits: f64,
    pub today_reward: f64,
    /// 近 14 天签到成功/已签账号数（趋势条）
    pub trend: Vec<Value>,
    pub recent: Vec<core::CheckinRecord>,
}

#[tauri::command]
pub fn autoclaw_dashboard(state: State<AppState>) -> Dashboard {
    let accounts = core::load_pool(&state);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let results = core::load_results(&state, 14);
    let checked_today = accounts
        .iter()
        .filter(|a| a.last_checkin_date == today && (a.last_checkin_status == "success" || a.last_checkin_status == "already"))
        .count();
    let today_reward: f64 = results
        .iter()
        .filter(|r| r.date == today)
        .filter_map(|r| r.reward)
        .sum();
    let mut trend = Vec::new();
    for i in (0..14).rev() {
        let date = (chrono::Local::now().date_naive() - chrono::Duration::days(i))
            .format("%Y-%m-%d")
            .to_string();
        let success = results
            .iter()
            .filter(|r| r.date == date && (r.status == "success" || r.status == "already"))
            .count();
        let failed = results.iter().filter(|r| r.date == date && r.status == "fail").count();
        trend.push(json!({ "date": date, "success": success, "failed": failed }));
    }
    let total_credits = accounts.iter().filter_map(|a| a.credits_balance).sum();
    Dashboard {
        accounts: accounts.len(),
        checked_today,
        total_credits,
        today_reward,
        trend,
        recent: results.into_iter().take(20).collect(),
    }
}

// ── 客户端 / 数据目录 ────────────────────────────────────────────────────

#[tauri::command(async)]
pub fn autoclaw_open_app(state: State<AppState>) -> Result<(), String> {
    let exe = app_path_of(&state).ok_or("未检测到 AutoClaw 客户端，请在环境配置页手动指定 exe 路径")?;
    std::process::Command::new(&exe)
        .spawn()
        .map_err(|e| format!("启动失败（{exe}）：{e}"))?;
    Ok(())
}

#[tauri::command(async)]
pub fn autoclaw_open_data_dir(state: State<AppState>) -> Result<(), String> {
    let dir = core::data_dir_of(&state).ok_or("未找到 AutoClaw 数据目录（可手动指定后重试）")?;
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map_err(|e| format!("打开目录失败：{e}"))?;
    Ok(())
}

#[tauri::command(async)]
pub fn autoclaw_open_site(region: Option<String>) -> Result<(), String> {
    let url = if region.as_deref() == Some("global") {
        "https://autoclaw.z.ai/"
    } else {
        "https://autoclaw.zhipuai.cn/"
    };
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map_err(|e| format!("打开官网失败：{e}"))?;
    Ok(())
}

// ── 计划任务（schtasks 直调主 exe --task-run autoclaw-checkin）────────────

fn extract_hhmm(text: &str) -> String {
    let b = text.as_bytes();
    let mut i = 0;
    while i + 5 <= b.len() {
        if b[i].is_ascii_digit()
            && b[i + 1].is_ascii_digit()
            && b[i + 2] == b':'
            && b[i + 3].is_ascii_digit()
            && b[i + 4].is_ascii_digit()
        {
            return text[i..i + 5].to_string();
        }
        i += 1;
    }
    String::new()
}

#[tauri::command(async)]
pub fn autoclaw_task_register(state: State<AppState>, time: String) -> Result<(), String> {
    crate::commands::misc::validate_hhmm(&time)?;
    let exe = std::env::current_exe().map_err(|e| format!("获取主程序路径失败：{e}"))?;
    let tr = format!("\"{}\" --task-run autoclaw-checkin", exe.to_string_lossy());
    let (ok, _out, err) = crate::commands::misc::run_schtasks(&[
        "/Create",
        "/TN",
        AUTOCLAW_TASK_NAME,
        "/TR",
        tr.as_str(),
        "/SC",
        "DAILY",
        "/ST",
        time.as_str(),
        "/F",
    ])?;
    if !ok {
        fs_utils::app_log(&state.data_dir, &format!("AutoClaw 计划任务注册失败：{err}"));
        return Err(err.trim().to_string());
    }
    crate::commands::misc::enable_task_wakeup_settings(AUTOCLAW_TASK_NAME);
    fs_utils::app_log(&state.data_dir, &format!("AutoClaw 定时签到已注册：{time}"));
    Ok(())
}

#[tauri::command(async)]
pub fn autoclaw_task_status() -> Result<String, String> {
    let (ok, stdout, _err) =
        crate::commands::misc::run_schtasks(&["/Query", "/TN", AUTOCLAW_TASK_NAME, "/FO", "LIST"])?;
    if !ok {
        return Ok("not_registered".into());
    }
    let time = extract_hhmm(&stdout);
    Ok(format!("registered:{time}"))
}

#[tauri::command(async)]
pub fn autoclaw_task_unregister(state: State<AppState>) -> Result<(), String> {
    let (ok, _out, err) =
        crate::commands::misc::run_schtasks(&["/Delete", "/TN", AUTOCLAW_TASK_NAME, "/F"])?;
    if !ok && !err.contains("不存在") && !err.contains("does not exist") {
        return Err(err.trim().to_string());
    }
    fs_utils::app_log(&state.data_dir, "AutoClaw 定时签到已注销");
    Ok(())
}
