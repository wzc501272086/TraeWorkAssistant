//! AutoClaw（智谱「澳龙」）账号池 / 每日签到 / 积分域。
//!
//! 端点情报（2026-09 桌面端 1.18.5 静态逆向 + 公开逆向备忘互证）：
//! - userapi 双区基址：
//!   cn     = `https://autoglm-acceleration-api.zhipuai.cn`
//!   global = `https://autoglm-api.autoglm.ai`
//! - 每日签到：`POST {base}/autoclaw-proxy/proxy/autoclaw-task-complete`
//!   body `{"task_id":"daily_signin"}`；幂等，`data.already_completed=true` = 今日已签，
//!   `data.reward_points` = 本次奖励积分（约 400/天）
//! - 任务列表：`GET {base}/autoclaw-proxy/proxy/autoclaw-task-list?lang=zh-CN`
//! - 积分钱包：`GET {base}/agent-assetmgr/api/v2/wallets?biz_app_id=autoclaw`
//!   → `data.total_balance` + `data.wallets[]`
//! - 公共头：`X-Version` / `X-Product: autoclaw` / `X-Client-Type: pc` /
//!   `X-Auth-Appid: 100003` / `X-Auth-TimeStamp: <unix 秒>` /
//!   `X-Auth-Sign: md5("100003&<ts>&<secret>")` / `X-Trace-Id` / `Authorization: Bearer`
//!
//! 凭证红线：accessToken / refreshToken 等同密码——只进 `autoclaw_tokens` 文档，
//! 不进日志、不进 NDJSON 事件、不进前端视图（视图只暴露 has_credential / uid 尾号）。
//!
//! 存储（全部走 kv，不新增表，避免 schema 迁移）：
//! - `autoclaw_pool`：`{ accounts: [AutoClawAccount] }`
//! - `autoclaw_tokens`：`{ accounts: { "<id>": { access_token, refresh_token } } }`
//! - `autoclaw_checkin_results`：`{ results: [...] }`（90 天滚动）
//! - `autoclaw_credits_history`：`{ snapshots: [...] }`
//! - `autoclaw_settings`：`AutoClawSettings`

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::AppState;

pub const CN_BASE: &str = "https://autoglm-acceleration-api.zhipuai.cn";
pub const GLOBAL_BASE: &str = "https://autoglm-api.autoglm.ai";
const APP_ID: &str = "100003";
/// 客户端内置签名盐（公开在桌面端 bundle 中，非用户凭证）
const APP_SECRET: &str = "38d2391985e2369a5fb8227d8e6cd5e5";
const CLIENT_VER: &str = "1.18.5";
const TASK_ID: &str = "daily_signin";
const CACHE_TTL_SECS: i64 = 600;

pub fn base_of(region: &str) -> &'static str {
    if region.eq_ignore_ascii_case("global") {
        GLOBAL_BASE
    } else {
        CN_BASE
    }
}

/// 默认签到时刻（AutoClaw 每日任务在北京时间 0 点刷新；避开整点风控高峰）
fn default_checkin_hhmm() -> String {
    "09:40".into()
}

fn default_true() -> bool {
    true
}

// ── 设置 ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
pub struct AutoClawSettings {
    /// 手动指定 AutoClaw 用户数据目录（空 = 按 APPDATA 候选自动探测）
    #[serde(default)]
    pub data_dir: String,
    /// 手动指定 AutoClaw 主程序路径（空 = 自动探测）
    #[serde(default)]
    pub app_path: String,
    /// 应用内「定时签到」开关（schtasks 计划任务 + 手动一键共用同一份账号池）
    #[serde(default = "default_true")]
    pub auto_checkin: bool,
    /// 定时签到时刻 HH:MM
    #[serde(default = "default_checkin_hhmm")]
    pub checkin_hhmm: String,
}

impl Default for AutoClawSettings {
    fn default() -> Self {
        Self {
            data_dir: String::new(),
            app_path: String::new(),
            auto_checkin: true,
            checkin_hhmm: default_checkin_hhmm(),
        }
    }
}

pub fn load_settings(state: &AppState) -> AutoClawSettings {
    crate::store::db(&state.data_dir).kv_get("autoclaw_settings")
}

pub fn save_settings(state: &AppState, s: &AutoClawSettings) -> Result<(), String> {
    crate::store::db(&state.data_dir).kv_set("autoclaw_settings", s)
}

// ── 账号池 ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AutoClawAccount {
    /// `ac-` + sha256(uid)[..12]
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub nickname: String,
    /// cn | global
    #[serde(default)]
    pub region: String,
    /// local_scan | token
    #[serde(default)]
    pub credential_source: String,
    #[serde(default)]
    pub token_expires_at: Option<i64>,
    #[serde(default)]
    pub needs_relogin: bool,
    #[serde(default)]
    pub relogin_reason: String,
    #[serde(default)]
    pub group_id: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub credits_balance: Option<f64>,
    #[serde(default)]
    pub credits_fetched_at: Option<String>,
    /// 最近一次签到日期（YYYY-MM-DD）与状态（success/already/fail）
    #[serde(default)]
    pub last_checkin_date: String,
    #[serde(default)]
    pub last_checkin_status: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct PoolDoc {
    #[serde(default)]
    accounts: Vec<AutoClawAccount>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct TokenEntry {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
}

static POOL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn load_pool(state: &AppState) -> Vec<AutoClawAccount> {
    let doc: PoolDoc = crate::store::db(&state.data_dir).kv_get("autoclaw_pool");
    doc.accounts
}

pub fn save_pool(state: &AppState, accounts: &[AutoClawAccount]) -> Result<(), String> {
    let doc = PoolDoc { accounts: accounts.to_vec() };
    crate::store::db(&state.data_dir).kv_set("autoclaw_pool", &doc)
}

/// 池读-改-写互斥（导入/改名/删除/积分回写/签到回写共用）
pub fn with_pool_mut<T>(
    state: &AppState,
    f: impl FnOnce(&mut Vec<AutoClawAccount>) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = POOL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut accounts = load_pool(state);
    let out = f(&mut accounts)?;
    save_pool(state, &accounts)?;
    Ok(out)
}

pub fn account_id_of(uid_or_token: &str) -> String {
    format!("ac-{}", &sha256_hex(uid_or_token)[..12])
}

// ── 凭证文档 ──────────────────────────────────────────────────────────────

fn tokens_doc(state: &AppState) -> Value {
    let v: Value = crate::store::db(&state.data_dir).kv_get("autoclaw_tokens");
    if v.is_object() {
        v
    } else {
        json!({ "accounts": {} })
    }
}

pub fn token_of(state: &AppState, id: &str) -> Option<TokenEntry> {
    let doc = tokens_doc(state);
    let entry = doc.get("accounts")?.get(id)?.clone();
    serde_json::from_value::<TokenEntry>(entry).ok().filter(|t| !t.access_token.trim().is_empty())
}

pub fn set_token(state: &AppState, id: &str, entry: &TokenEntry) -> Result<(), String> {
    let mut doc = tokens_doc(state);
    let obj = doc
        .get_mut("accounts")
        .and_then(Value::as_object_mut)
        .ok_or("token 文档结构异常")?;
    obj.insert(id.to_string(), serde_json::to_value(entry).map_err(|e| e.to_string())?);
    crate::store::db(&state.data_dir).kv_set("autoclaw_tokens", &doc)
}

pub fn remove_token(state: &AppState, id: &str) -> Result<(), String> {
    let mut doc = tokens_doc(state);
    if let Some(obj) = doc.get_mut("accounts").and_then(Value::as_object_mut) {
        obj.remove(id);
    }
    crate::store::db(&state.data_dir).kv_set("autoclaw_tokens", &doc)
}

// ── 小工具 ────────────────────────────────────────────────────────────────

fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

pub fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn now_ts() -> i64 {
    chrono::Local::now().timestamp()
}

fn mask_secret(s: &str) -> String {
    let t = s.trim();
    let chars: Vec<char> = t.chars().collect();
    if chars.len() <= 8 {
        "****".into()
    } else {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}****{tail}")
    }
}

/// JWT 载荷（不校验签名，仅取 uid / 昵称等展示字段；失败 → Null）
pub fn jwt_claims(token: &str) -> Value {
    let Some(payload) = token.split('.').nth(1) else {
        return Value::Null;
    };
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(payload));
    let Ok(bytes) = raw else {
        return Value::Null;
    };
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn claim_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(Value::as_str) {
            if !s.trim().is_empty() {
                return s.trim().to_string();
            }
        }
        if let Some(n) = v.get(*k).and_then(Value::as_i64) {
            return n.to_string();
        }
    }
    String::new()
}

/// 从 JWT 提 uid（AutoClaw accessToken 为 JWT 形态；字段名随版本有差异，宽松匹配）
pub fn uid_of_token(token: &str) -> String {
    let c = jwt_claims(token);
    let uid = claim_str(&c, &["user_id", "userId", "uid", "sub"]);
    if !uid.is_empty() {
        return uid;
    }
    // 无可用 JWT 载荷时退化为 token 指纹（同 token 稳定同 id，防重复入池）
    format!("token-{}", &sha256_hex(token.trim())[..12])
}

pub fn nickname_of_token(token: &str) -> String {
    let c = jwt_claims(token);
    claim_str(&c, &["user_name", "userName", "nickname", "name"])
}

fn expire_of_token(token: &str) -> Option<i64> {
    let c = jwt_claims(token);
    c.get("exp").and_then(Value::as_i64)
}

fn bearer(token: &str) -> String {
    let t = token.trim();
    if t.to_ascii_lowercase().starts_with("bearer ") {
        t.to_string()
    } else {
        format!("Bearer {t}")
    }
}

fn headers(token: &str) -> Vec<(String, String)> {
    let ts = now_ts().to_string();
    let sign = md5_hex(&format!("{APP_ID}&{ts}&{APP_SECRET}"));
    vec![
        ("Content-Type".into(), "application/json".into()),
        ("Accept".into(), "application/json".into()),
        ("X-Version".into(), CLIENT_VER.into()),
        ("X-Tm".into(), "mac".into()),
        ("X-Product".into(), "autoclaw".into()),
        ("X-Client-Type".into(), "pc".into()),
        ("X-Channel".into(), "official".into()),
        ("X-Auth-Appid".into(), APP_ID.into()),
        ("X-Auth-TimeStamp".into(), ts),
        ("X-Auth-Sign".into(), sign),
        ("X-Trace-Id".into(), uuid::Uuid::new_v4().to_string()),
        ("X-Lang".into(), "zh-CN".into()),
        ("Authorization".into(), bearer(token)),
        ("User-Agent".into(), format!("AutoClaw/{CLIENT_VER}")),
    ]
}

/// 统一 JSON 请求：返回 (HTTP 状态码, 解析后的 JSON)；网络层失败 → Err
pub fn http_json(
    method: &str,
    url: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    let agent = crate::tasks::http_agent(30);
    let mut req = agent.request(method, url);
    for (k, v) in headers(token) {
        req = req.set(&k, &v);
    }
    let res = match body {
        Some(b) => req.send_json(b.clone()),
        None => req.call(),
    };
    match res {
        Ok(r) => {
            let status = r.status();
            let text = r.into_string().unwrap_or_default();
            Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
        }
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            Ok((code, serde_json::from_str(&text).unwrap_or(Value::Null)))
        }
        Err(e) => Err(format!("网络请求失败：{e}")),
    }
}

fn api_error(status: u16, v: &Value) -> Option<String> {
    let code = v.get("code").and_then(Value::as_i64);
    if code == Some(410000) || status == 401 {
        return Some("登录态失效（401/410000），请重新登录该账号".into());
    }
    if status >= 400 && code.is_none() {
        return Some(format!("HTTP {status}: {}", v.get("msg").and_then(Value::as_str).unwrap_or("请求失败")));
    }
    if let Some(c) = code {
        if c != 0 {
            return Some(format!("code={c} {}", v.get("msg").and_then(Value::as_str).unwrap_or("")));
        }
    }
    None
}

// ── 积分 ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct WalletSlice {
    pub kind: String,
    pub balance: f64,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CreditsInfo {
    pub ok: bool,
    pub total_balance: f64,
    pub wallets: Vec<WalletSlice>,
    pub message: String,
    pub fetched_at: i64,
}

fn number_of(v: &Value, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(n) = v.get(*k).and_then(Value::as_f64) {
            return Some(n);
        }
        if let Some(s) = v.get(*k).and_then(Value::as_str) {
            if let Ok(n) = s.trim().parse::<f64>() {
                return Some(n);
            }
        }
    }
    None
}

pub fn fetch_credits(token: &str, region: &str) -> CreditsInfo {
    let url = format!("{}/agent-assetmgr/api/v2/wallets?biz_app_id=autoclaw", base_of(region));
    let now = now_ts();
    let (status, v) = match http_json("GET", &url, token, None) {
        Ok(r) => r,
        Err(e) => {
            return CreditsInfo {
                ok: false,
                message: e,
                fetched_at: now,
                ..Default::default()
            }
        }
    };
    if let Some(err) = api_error(status, &v) {
        return CreditsInfo { ok: false, message: err, fetched_at: now, ..Default::default() };
    }
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    let mut wallets = Vec::new();
    if let Some(arr) = data.get("wallets").and_then(Value::as_array) {
        for w in arr {
            let kind = claim_str(w, &["wallet_type", "walletType", "type", "biz_type"]);
            let balance = number_of(w, &["balance", "available_balance", "amount", "total_balance"])
                .unwrap_or(0.0);
            if !kind.is_empty() || balance != 0.0 {
                wallets.push(WalletSlice { kind: if kind.is_empty() { "other".into() } else { kind }, balance });
            }
        }
    }
    let total = number_of(&data, &["total_balance", "totalBalance", "balance"])
        .unwrap_or_else(|| wallets.iter().map(|w| w.balance).sum());
    CreditsInfo { ok: true, total_balance: total, wallets, message: String::new(), fetched_at: now }
}

// ── 签到 ──────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct CheckinOutcome {
    /// success | already | fail
    pub kind: String,
    pub reward: Option<f64>,
    pub message: String,
    pub needs_relogin: bool,
}

pub fn checkin_one(token: &str, region: &str) -> CheckinOutcome {
    let url = format!("{}/autoclaw-proxy/proxy/autoclaw-task-complete", base_of(region));
    let body = json!({ "task_id": TASK_ID });
    let (status, v) = match http_json("POST", &url, token, Some(&body)) {
        Ok(r) => r,
        Err(e) => {
            return CheckinOutcome { kind: "fail".into(), message: e, ..Default::default() }
        }
    };
    let code = v.get("code").and_then(Value::as_i64);
    let data = v.get("data").cloned().unwrap_or(Value::Null);
    let msg = v.get("msg").and_then(Value::as_str).unwrap_or("").to_string();
    if code == Some(410000) || status == 401 {
        return CheckinOutcome {
            kind: "fail".into(),
            message: "登录态失效，请重新登录".into(),
            needs_relogin: true,
            ..Default::default()
        };
    }
    // code=0 成功；code=null 且有 data 也算成功（与桌面端 completeClientTask 一致）
    let ok = code == Some(0) || (code.is_none() && data.is_object());
    if !ok {
        return CheckinOutcome {
            kind: "fail".into(),
            message: if msg.is_empty() { format!("code={code:?}") } else { msg },
            ..Default::default()
        };
    }
    let already = data.get("already_completed").and_then(Value::as_bool).unwrap_or(false);
    let reward = number_of(&data, &["reward_points", "rewardPoints", "reward"]);
    CheckinOutcome {
        kind: if already { "already".into() } else { "success".into() },
        reward,
        message: if already {
            "今日已签到（幂等回放）".into()
        } else {
            match reward {
                Some(r) => format!("签到成功，+{} 积分", r as i64),
                None => "签到成功".into(),
            }
        },
        needs_relogin: false,
    }
}

#[derive(Serialize, Clone)]
pub struct CheckinRecord {
    pub date: String,
    pub time: String,
    pub user_id: String,
    pub name: String,
    pub region: String,
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<f64>,
}

fn results_doc(state: &AppState) -> Value {
    let v: Value = crate::store::db(&state.data_dir).kv_get("autoclaw_checkin_results");
    if v.is_object() {
        v
    } else {
        json!({ "results": [] })
    }
}

pub fn load_results(state: &AppState, days: i64) -> Vec<CheckinRecord> {
    let cutoff = (chrono::Local::now().date_naive() - chrono::Duration::days(days.clamp(1, 90)))
        .format("%Y-%m-%d")
        .to_string();
    let doc = results_doc(state);
    let mut out = Vec::new();
    if let Some(arr) = doc.get("results").and_then(Value::as_array) {
        for r in arr {
            let date = r.get("date").and_then(Value::as_str).unwrap_or("");
            if date < cutoff.as_str() {
                continue;
            }
            out.push(CheckinRecord {
                date: date.into(),
                time: r.get("time").and_then(Value::as_str).unwrap_or("").into(),
                user_id: r.get("user_id").and_then(Value::as_str).unwrap_or("").into(),
                name: r.get("name").and_then(Value::as_str).unwrap_or("").into(),
                region: r.get("region").and_then(Value::as_str).unwrap_or("cn").into(),
                status: r.get("status").and_then(Value::as_str).unwrap_or("").into(),
                message: r.get("message").and_then(Value::as_str).unwrap_or("").into(),
                reward: r.get("reward").and_then(Value::as_f64),
            });
        }
    }
    out.sort_by(|a, b| (b.date.as_str(), b.time.as_str()).cmp(&(a.date.as_str(), a.time.as_str())));
    out
}

fn append_results(state: &AppState, rows: &[CheckinRecord]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut doc = results_doc(state);
    let cutoff = (chrono::Local::now().date_naive() - chrono::Duration::days(90))
        .format("%Y-%m-%d")
        .to_string();
    let mut all: Vec<Value> = Vec::new();
    if let Some(arr) = doc.get("results").and_then(Value::as_array) {
        for r in arr {
            let date = r.get("date").and_then(Value::as_str).unwrap_or("");
            if date >= cutoff.as_str() {
                all.push(r.clone());
            }
        }
    }
    for r in rows {
        all.push(json!({
            "date": r.date, "time": r.time, "user_id": r.user_id, "name": r.name,
            "region": r.region, "status": r.status, "message": r.message, "reward": r.reward,
        }));
    }
    doc["results"] = Value::Array(all);
    crate::store::db(&state.data_dir).kv_set("autoclaw_checkin_results", &doc)
}

fn update_account_checkin(state: &AppState, results: &[(String, CheckinOutcome)]) {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _ = with_pool_mut(state, |accounts| {
        for (id, out) in results {
            if let Some(a) = accounts.iter_mut().find(|a| &a.id == id) {
                a.last_checkin_date = date.clone();
                a.last_checkin_status = out.kind.clone();
                if out.needs_relogin {
                    a.needs_relogin = true;
                    a.relogin_reason = "签到返回登录态失效".into();
                }
            }
        }
        Ok(())
    });
}

/// 单轮签到（UI 与 CLI 共用）：逐账号执行并对每行回调，返回汇总
pub fn run_round(
    state: &AppState,
    uids: &[String],
    regions: &[String],
    sink: &mut dyn FnMut(&Value),
) -> Value {
    let accounts = load_pool(state);
    let targets: Vec<AutoClawAccount> = accounts
        .into_iter()
        .filter(|a| uids.is_empty() || uids.iter().any(|u| u == &a.id || u == &a.uid))
        .filter(|a| {
            regions.is_empty()
                || regions.iter().any(|r| {
                    let r = r.to_ascii_lowercase();
                    r == "all" || r == if a.region.is_empty() { "cn" } else { a.region.as_str() }
                })
        })
        .collect();
    if targets.is_empty() {
        sink(&json!({ "type": "done", "ok": true, "total": 0, "ok_count": 0, "already": 0, "failed": 0, "skipped_empty_pool": true }));
        return json!({ "ok": true, "total": 0, "ok_count": 0, "already": 0, "failed": 0, "skipped_empty_pool": true });
    }
    sink(&json!({ "type": "start", "total": targets.len(), "time": chrono::Local::now().format("%H:%M:%S").to_string() }));

    let mut ok_count = 0i64;
    let mut already = 0i64;
    let mut failed = 0i64;
    let mut rows: Vec<CheckinRecord> = Vec::new();
    let mut outcome_pairs: Vec<(String, CheckinOutcome)> = Vec::new();

    for a in &targets {
        let Some(tok) = token_of(state, &a.id) else {
            failed += 1;
            let out = CheckinOutcome {
                kind: "fail".into(),
                message: "无可用凭证（本机登录态未捕获 / token 已删除）".into(),
                ..Default::default()
            };
            sink(&json!({
                "type": "line", "stage": "checkin", "status": "fail",
                "message": format!("{}（{}）：{}", a.nickname, a.uid, out.message),
                "uid": a.uid, "name": a.nickname,
                "time": chrono::Local::now().format("%H:%M:%S").to_string(),
            }));
            rows.push(record_of(a, &out));
            outcome_pairs.push((a.id.clone(), out));
            continue;
        };
        let out = checkin_one(&tok.access_token, &a.region);
        match out.kind.as_str() {
            "success" => ok_count += 1,
            "already" => already += 1,
            _ => failed += 1,
        }
        let status = if out.kind == "success" { "ok" } else { out.kind.as_str() };
        sink(&json!({
            "type": "line", "stage": "checkin", "status": status,
            "message": format!("{}（{}）：{}", a.nickname, a.uid, out.message),
            "uid": a.uid, "name": a.nickname, "region": a.region, "reward": out.reward,
            "time": chrono::Local::now().format("%H:%M:%S").to_string(),
        }));
        rows.push(record_of(a, &out));
        outcome_pairs.push((a.id.clone(), out));
    }

    update_account_checkin(state, &outcome_pairs);
    if let Err(e) = append_results(state, &rows) {
        crate::fs_utils::app_log(&state.data_dir, &format!("AutoClaw 签到结果落库失败：{e}"));
    }
    let summary = json!({
        "ok": failed == 0,
        "total": targets.len(),
        "ok_count": ok_count,
        "already": already,
        "failed": failed,
    });
    sink(&json!({
        "type": "done", "ok": failed == 0, "total": targets.len(),
        "ok_count": ok_count, "already": already, "failed": failed,
    }));
    summary
}

fn record_of(a: &AutoClawAccount, out: &CheckinOutcome) -> CheckinRecord {
    CheckinRecord {
        date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        time: chrono::Local::now().format("%H:%M:%S").to_string(),
        user_id: a.id.clone(),
        name: a.nickname.clone(),
        region: a.region.clone(),
        status: out.kind.clone(),
        message: out.message.clone(),
        reward: out.reward,
    }
}

// ── 积分批量 / 历史 ───────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct CreditsAccountView {
    pub user_id: String,
    pub name: String,
    pub region: String,
    pub ok: bool,
    pub total_balance: f64,
    pub wallets: Vec<WalletSlice>,
    pub message: String,
    pub cached: bool,
    pub fetched_at: i64,
}

pub fn credits_fetch(state: &AppState, uid: Option<&str>, fresh: bool) -> Value {
    let accounts = load_pool(state);
    let targets: Vec<AutoClawAccount> = accounts
        .into_iter()
        .filter(|a| uid.map(|u| u == a.id || u == a.uid).unwrap_or(true))
        .collect();
    let cache_doc: Value = crate::store::db(&state.data_dir).kv_get("autoclaw_credits_cache");
    let mut cache = cache_doc.get("by_id").cloned().unwrap_or_else(|| json!({}));
    let now = now_ts();
    let mut views: Vec<CreditsAccountView> = Vec::new();
    let mut balances: Vec<(String, f64)> = Vec::new();

    for a in &targets {
        let cached = cache.get(a.id.as_str()).cloned().unwrap_or(Value::Null);
        let cached_at = cached.get("ts").and_then(Value::as_i64).unwrap_or(0);
        if !fresh && now - cached_at < CACHE_TTL_SECS && cached.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let info: CreditsInfo = serde_json::from_value(
                cached.get("info").cloned().unwrap_or(Value::Null),
            )
            .unwrap_or_default();
            views.push(CreditsAccountView {
                user_id: a.uid.clone(),
                name: a.nickname.clone(),
                region: a.region.clone(),
                ok: true,
                total_balance: info.total_balance,
                wallets: info.wallets,
                message: String::new(),
                cached: true,
                fetched_at: cached_at,
            });
            balances.push((a.id.clone(), info.total_balance));
            continue;
        }
        let Some(tok) = token_of(state, &a.id) else {
            views.push(CreditsAccountView {
                user_id: a.uid.clone(),
                name: a.nickname.clone(),
                region: a.region.clone(),
                ok: false,
                total_balance: 0.0,
                wallets: Vec::new(),
                message: "无可用凭证".into(),
                cached: false,
                fetched_at: now,
            });
            continue;
        };
        let info = fetch_credits(&tok.access_token, &a.region);
        if let Some(obj) = cache.as_object_mut() {
            obj.insert(
                a.id.clone(),
                json!({ "ts": info.fetched_at, "ok": info.ok, "info": info }),
            );
        }
        if info.ok {
            balances.push((a.id.clone(), info.total_balance));
        }
        views.push(CreditsAccountView {
            user_id: a.uid.clone(),
            name: a.nickname.clone(),
            region: a.region.clone(),
            ok: info.ok,
            total_balance: info.total_balance,
            wallets: info.wallets,
            message: info.message,
            cached: false,
            fetched_at: info.fetched_at,
        });
    }

    let _ = crate::store::db(&state.data_dir).kv_set("autoclaw_credits_cache", &json!({ "by_id": cache }));

    let fetched_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if !balances.is_empty() {
        let _ = with_pool_mut(state, |accounts| {
            for (id, bal) in &balances {
                if let Some(a) = accounts.iter_mut().find(|a| &a.id == id) {
                    a.credits_balance = Some(*bal);
                    a.credits_fetched_at = Some(fetched_at.clone());
                }
            }
            Ok(())
        });
        append_snapshot(state, &balances);
    }
    let total: f64 = views.iter().filter(|v| v.ok).map(|v| v.total_balance).sum();
    json!({
        "ok": views.iter().any(|v| v.ok),
        "cached": views.iter().all(|v| v.cached) && !views.is_empty(),
        "accounts": views,
        "total_balance": total,
        "fetched_at": fetched_at,
    })
}

fn append_snapshot(state: &AppState, balances: &[(String, f64)]) {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut doc: Value = crate::store::db(&state.data_dir).kv_get("autoclaw_credits_history");
    if !doc.is_object() {
        doc = json!({ "snapshots": [] });
    }
    let mut arr: Vec<Value> = doc
        .get("snapshots")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total: f64 = balances.iter().map(|(_, b)| *b).sum();
    let entry = json!({
        "date": date,
        "total": total,
        "accounts": balances.iter().map(|(id, b)| json!({ "id": id, "balance": b })).collect::<Vec<_>>(),
    });
    if let Some(existing) = arr.iter_mut().find(|s| s.get("date").and_then(Value::as_str) == Some(date.as_str())) {
        *existing = entry;
    } else {
        arr.push(entry);
    }
    arr.sort_by(|a, b| a.get("date").and_then(Value::as_str).cmp(&b.get("date").and_then(Value::as_str)));
    let keep = arr.len().saturating_sub(365);
    if keep > 0 {
        arr.drain(0..keep);
    }
    doc["snapshots"] = Value::Array(arr);
    let _ = crate::store::db(&state.data_dir).kv_set("autoclaw_credits_history", &doc);
}

pub fn credits_history(state: &AppState) -> Vec<Value> {
    let doc: Value = crate::store::db(&state.data_dir).kv_get("autoclaw_credits_history");
    doc.get("snapshots").and_then(Value::as_array).cloned().unwrap_or_default()
}

// ── 本机凭据扫描 ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ScanHit {
    pub uid: String,
    pub nickname: String,
    pub region: String,
    pub access_token: String,
    pub refresh_token: String,
    pub source_path: String,
}

/// AutoClaw 用户数据目录候选（settings 手动路径优先；其余按 Electron 默认 userData 规则）
pub fn data_dir_of(state: &AppState) -> Option<PathBuf> {
    let s = load_settings(state);
    let mut cands: Vec<PathBuf> = Vec::new();
    if !s.data_dir.trim().is_empty() {
        cands.push(PathBuf::from(s.data_dir.trim()));
    }
    for key in ["APPDATA", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(key) {
            for name in ["AutoClaw-official", "AutoClaw", "AutoClaw2", "autoclaw"] {
                cands.push(PathBuf::from(&base).join(name));
            }
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        cands.push(PathBuf::from(&home).join(".openclaw-autoclaw"));
        cands.push(PathBuf::from(&home).join(".autoclaw"));
    }
    cands.into_iter().find(|p| p.join("Local State").is_file() || p.join("accounts").is_dir())
}

/// 扫本机 AutoClaw 账号目录（`accounts/<id>/account-credentials.enc`，
/// Chromium os_crypt v10：Local State encrypted_key → DPAPI → AES-256-GCM）
pub fn scan_local(data_dir: &Path) -> Vec<ScanHit> {
    #[cfg(windows)]
    {
        scan_local_impl(data_dir)
    }
    #[cfg(not(windows))]
    {
        let _ = data_dir;
        Vec::new()
    }
}

#[cfg(windows)]
fn scan_local_impl(data_dir: &Path) -> Vec<ScanHit> {
    let Some(key) = local_state_key(data_dir) else {
        return Vec::new();
    };
    let accounts_dir = data_dir.join("accounts");
    let Ok(entries) = std::fs::read_dir(&accounts_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten() {
        let dir = ent.path();
        if !dir.is_dir() {
            continue;
        }
        let dir_uid = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let mut files: Vec<PathBuf> = Vec::new();
        if let Ok(fs) = std::fs::read_dir(&dir) {
            for f in fs.flatten() {
                let p = f.path();
                if p.is_file() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
                    if name.contains("credential") || name.ends_with(".json") {
                        files.push(p);
                    }
                }
            }
        }
        for f in files {
            let Ok(enc) = std::fs::read(&f) else { continue };
            let Some(text) = decrypt_credential(&enc, &key) else { continue };
            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            let token = claim_str(&v, &["accessToken", "access_token", "token"]);
            if token.is_empty() {
                continue;
            }
            let uid = {
                let from_file = claim_str(&v, &["userId", "user_id", "uid"]);
                if !from_file.is_empty() {
                    from_file
                } else if !dir_uid.is_empty() {
                    dir_uid.clone()
                } else {
                    uid_of_token(&token)
                }
            };
            let nickname = {
                let n = claim_str(&v, &["userName", "user_name", "nickname", "name"]);
                if n.is_empty() {
                    nickname_of_token(&token)
                } else {
                    n
                }
            };
            let region = {
                let r = claim_str(&v, &["region"]);
                if r.is_empty() { "cn".into() } else { r.to_ascii_lowercase() }
            };
            out.push(ScanHit {
                uid,
                nickname,
                region,
                access_token: token,
                refresh_token: claim_str(&v, &["refreshToken", "refresh_token"]),
                source_path: f.to_string_lossy().to_string(),
            });
            break;
        }
    }
    out
}

#[cfg(windows)]
fn local_state_key(data_dir: &Path) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let raw = std::fs::read_to_string(data_dir.join("Local State")).ok()?;
    let state: Value = serde_json::from_str(&raw).ok()?;
    let b64 = state.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if bytes.len() < 5 || &bytes[..5] != b"DPAPI" {
        return None;
    }
    crate::vault::dpapi::unprotect(&bytes[5..]).ok()
}

#[cfg(windows)]
fn decrypt_credential(enc: &[u8], key: &[u8]) -> Option<String> {
    if enc.len() > 19 && &enc[..3] == b"v10" {
        use aes_gcm::aead::Aead;
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        let cipher = Aes256Gcm::new_from_slice(key).ok()?;
        let plain = cipher.decrypt(Nonce::from_slice(&enc[3..15]), &enc[15..]).ok()?;
        String::from_utf8(plain).ok()
    } else {
        String::from_utf8(enc.to_vec()).ok()
    }
}

// ── 视图组装 ──────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct AccountView {
    pub id: String,
    pub uid: String,
    pub nickname: String,
    pub region: String,
    pub credential_source: String,
    pub token_hint: String,
    pub has_credential: bool,
    pub needs_relogin: bool,
    pub relogin_reason: String,
    pub group_id: String,
    pub note: String,
    pub credits_balance: Option<f64>,
    pub credits_fetched_at: Option<String>,
    pub last_checkin_date: String,
    pub last_checkin_status: String,
}

/// uid 尾号提示（按字符截取，避免多字节边界 panic）
fn uid_tail(uid: &str, n: usize) -> String {
    let chars: Vec<char> = uid.chars().collect();
    if chars.len() <= n {
        return uid.to_string();
    }
    format!("…{}", chars[chars.len() - n..].iter().collect::<String>())
}

pub fn view_of(a: &AutoClawAccount, has_credential: bool) -> AccountView {
    AccountView {
        id: a.id.clone(),
        uid: a.uid.clone(),
        nickname: a.nickname.clone(),
        region: if a.region.is_empty() { "cn".into() } else { a.region.clone() },
        credential_source: a.credential_source.clone(),
        // 视图只给 uid 尾号提示，绝不回传 token 本体
        token_hint: uid_tail(&a.uid, 6),
        has_credential,
        needs_relogin: a.needs_relogin,
        relogin_reason: a.relogin_reason.clone(),
        group_id: a.group_id.clone(),
        note: a.note.clone(),
        credits_balance: a.credits_balance,
        credits_fetched_at: a.credits_fetched_at.clone(),
        last_checkin_date: a.last_checkin_date.clone(),
        last_checkin_status: a.last_checkin_status.clone(),
    }
}

/// token 展示兜底（仅在用户主动粘贴导入时进入日志以外的调试文案；生产视图不用）
pub fn token_debug_hint(token: &str) -> String {
    mask_secret(token)
}

// ── 单元测试（纯函数） ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_is_md5_of_appid_ts_secret() {
        assert_eq!(md5_hex("100003&1700000000&38d2391985e2369a5fb8227d8e6cd5e5").len(), 32);
    }

    #[test]
    fn account_id_is_stable_and_prefixed() {
        let a = account_id_of("user-123");
        let b = account_id_of("user-123");
        assert_eq!(a, b);
        assert!(a.starts_with("ac-"));
        assert_eq!(a.len(), 15);
    }

    #[test]
    fn base_region_switch() {
        assert!(base_of("cn").contains("zhipuai.cn"));
        assert!(base_of("global").contains("autoglm.ai"));
        assert!(base_of("GLOBAL").contains("autoglm.ai"));
    }

    #[test]
    fn jwt_uid_extracts_common_claims() {
        // {"user_id":"u1"} → eyJ1c2VyX2lkIjoidTEifQ
        let token = "x.eyJ1c2VyX2lkIjoidTEifQ.sig";
        assert_eq!(uid_of_token(token), "u1");
    }

    #[test]
    fn mask_hides_middle() {
        assert_eq!(mask_secret("abcdefghijkl"), "abcd****ijkl");
    }
}
