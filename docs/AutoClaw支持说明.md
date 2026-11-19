# AutoClaw（智谱「澳龙」）支持说明

> 状态：v1（2026-11-19）已接入账号池 / 一键签到 / 积分看板 / 环境配置 / 定时签到。
> 平台：Windows（凭据解密依赖 DPAPI；macOS 预留 Keychain 分支未实现）。

## 1. 产品与接入形态

AutoClaw 是智谱推出的桌面 AI Agent（国内 `autoclaw.zhipuai.cn`，海外 `autoclaw.z.ai`），
登录后每日可领约 400 积分（`daily_signin` 任务），另有活动期登录奖励。

本应用以**独立应用 Tab**接入（与 Trae / Buddy / Qoder / 豆包 / BuddyAI 同级），
五个页面：概述、账号管理、一键签到、积分看板、环境配置。

**账号切换不在本期范围**：AutoClaw 客户端以 `accounts/<id>/` 目录管理登录态（含当前账号指针），
换号机制未公开，本应用不改写其凭据目录，避免破坏客户端登录态。多账号的签到与积分查询不受影响。

## 2. 接口基线（2026-09 桌面端 1.18.5 逆向，公开备忘互证）

| 用途 | 方法 | 端点 |
|---|---|---|
| 每日签到 | POST | `{base}/autoclaw-proxy/proxy/autoclaw-task-complete`，body `{"task_id":"daily_signin"}` |
| 任务列表 | GET | `{base}/autoclaw-proxy/proxy/autoclaw-task-list?lang=zh-CN` |
| 积分钱包 | GET | `{base}/agent-assetmgr/api/v2/wallets?biz_app_id=autoclaw` |

双区基址：

- 国内：`https://autoglm-acceleration-api.zhipuai.cn`
- 海外：`https://autoglm-api.autoglm.ai`

公共请求头：

```
X-Version: 1.18.5
X-Tm: mac                       # 桌面端形态
X-Product: autoclaw
X-Client-Type: pc
X-Channel: official
X-Auth-Appid: 100003
X-Auth-TimeStamp: <unix 秒>
X-Auth-Sign: md5("100003&<ts>&38d2391985e2369a5fb8227d8e6cd5e5")
X-Trace-Id: <uuid>
X-Lang: zh-CN
Authorization: Bearer <accessToken>
```

签到响应：`data.already_completed=true` 表示今日已签（幂等），`data.reward_points` 为本次奖励。
错误码 `410000` / HTTP 401 = 登录态失效。

## 3. 本机凭据扫描

- 数据目录候选（按顺序）：设置里的手动路径 → `%APPDATA%\AutoClaw-official` →
  `%APPDATA%\AutoClaw` / `AutoClaw2` → `%LOCALAPPDATA%\...` → `~/.openclaw-autoclaw`。
- 凭据文件：`accounts/<id>/account-credentials.enc`，Chromium `v10` 密文
  （`v10` + 12B nonce + AES-256-GCM）。
- 密钥：数据目录根 `Local State` 的 `os_crypt.encrypted_key`（base64，去掉 `DPAPI` 前缀）
  → `CryptUnprotectData` → 32 字节 AES 主密钥。
- 仅在**同一 Windows 用户**下可解；换用户 / 换机器解不开属预期（safeStorage 设计）。

扫描失败的两类常见原因：

1. 只有账号目录、没有 `account-credentials.enc`：该账号在 AutoClaw 里从未真正登录；
2. token 过期：重开 AutoClaw（凭据会被重写）→ 再扫描。

## 4. 存储

全部走 kv 文档，不新增 SQLite 表（避免 schema 迁移）：

| kv key | 内容 |
|---|---|
| `autoclaw_pool` | `{ accounts: [AutoClawAccount] }` |
| `autoclaw_tokens` | `{ accounts: { "<id>": { access_token, refresh_token } } }` |
| `autoclaw_checkin_results` | 签到记录（90 天滚动） |
| `autoclaw_credits_history` | 每日余额快照（365 天） |
| `autoclaw_credits_cache` | 600s 查询缓存 |
| `autoclaw_settings` | 数据目录 / 客户端路径 / 定时开关 / 时刻 |

凭证红线：token 只进 `autoclaw_tokens`，不进日志、不进 NDJSON 事件、不进前端视图
（视图只回 `has_credential` 与 uid 尾号）。

## 5. 定时签到

- Windows 计划任务名：`AIWorkAssistant_AutoClawCheckin`，`/TR` 直调主 exe
  `--task-run autoclaw-checkin`（GUI 子系统，无黑框）。
- CLI 分支在 `tasks::run_cli_task`，空池返回 `skipped_empty_pool`，不记失败。
- UI 手动签到与计划任务共用后端轮次互斥（`ROUND_BUSY`），并发时幂等跳过。

## 6. 已知边界

- 不提供批量注册、验证码绕过、风控规避能力。
- 不接入 API 网关（AutoClaw 对话通道为云端沙箱 + WebSocket 设备上线，接入成本与风险不在本期）。
- 接口为逆向所得，可能随客户端版本变更失效；失败会显式提示，不静默重试轰炸。
- 后端 Rust 代码在本机未经过 `cargo build`（开发环境无 Rust/MSVC 工具链）；
  前端已通过 `tsc --noEmit`、`vite build`、`vitest`（101 项）。首次构建如有编译错误请提交 issue。

## 7. 关键文件

| 层 | 文件 |
|---|---|
| 后端域 | `src-tauri/src/tasks/autoclaw.rs`（端点 / 签名 / 账号池 / 签到 / 积分 / 扫描） |
| 命令层 | `src-tauri/src/commands/autoclaw.rs`（19 个 `autoclaw_*` 命令 + schtasks） |
| CLI | `src-tauri/src/tasks/mod.rs`（`autoclaw-checkin` 分支） |
| 前端页面 | `src/pages/autoclaw/*`（5 页 + shared） |
| 注册点 | `types.ts` / `lib/tauri.ts` / `lib/appVisibility.ts` / `components/Sidebar.tsx` / `App.tsx` |
