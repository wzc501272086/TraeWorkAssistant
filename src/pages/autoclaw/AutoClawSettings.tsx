import { useCallback, useEffect, useState } from 'react';
import {
  AlertTriangle,
  Bell,
  CheckCircle2,
  ExternalLink,
  FolderOpen,
  RefreshCw,
  Save,
  Timer,
} from 'lucide-react';
import PageHeader from '../../components/PageHeader';
import { Badge, Spinner } from '../../components/ui';
import { api } from '../../lib/tauri';
import { useAppStore } from '../../store';
import type { AutoClawEnvCheck, AutoClawSettings } from '../../types';

export default function AutoClawSettings() {
  const pushToast = useAppStore((s) => s.pushToast);
  const [settings, setSettings] = useState<AutoClawSettings | null>(null);
  const [env, setEnv] = useState<AutoClawEnvCheck | null>(null);
  const [taskStatus, setTaskStatus] = useState('not_registered');
  const [saving, setSaving] = useState(false);
  const [registering, setRegistering] = useState(false);

  const load = useCallback(async () => {
    try {
      const [s, e, t] = await Promise.all([
        api.autoclaw.settingsGet(),
        api.autoclaw.envCheck(),
        api.autoclaw.taskStatus(),
      ]);
      setSettings(s);
      setEnv(e);
      setTaskStatus(t);
    } catch (err) {
      pushToast('error', `读取配置失败：${String(err)}`);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const save = async () => {
    if (!settings) return;
    setSaving(true);
    try {
      await api.autoclaw.settingsSet(settings);
      pushToast('success', '配置已保存');
      await load();
    } catch (err) {
      pushToast('error', String(err));
    } finally {
      setSaving(false);
    }
  };

  const registerTask = async () => {
    if (!settings) return;
    setRegistering(true);
    try {
      await api.autoclaw.taskRegister(settings.checkin_hhmm);
      pushToast('success', `定时签到已注册：每日 ${settings.checkin_hhmm}`);
      await load();
    } catch (err) {
      pushToast('error', `注册失败：${String(err)}`);
    } finally {
      setRegistering(false);
    }
  };

  const unregisterTask = async () => {
    setRegistering(true);
    try {
      await api.autoclaw.taskUnregister();
      pushToast('success', '定时任务已注销');
      await load();
    } catch (err) {
      pushToast('error', String(err));
    } finally {
      setRegistering(false);
    }
  };

  const registeredTime = taskStatus.startsWith('registered:') ? taskStatus.slice('registered:'.length) : '';
  const registered = taskStatus.startsWith('registered:');

  if (!settings) {
    return (
      <div className="flex h-40 items-center justify-center text-slate-400">
        <Spinner className="mr-2" /> 读取配置…
      </div>
    );
  }

  return (
    <div>
      <PageHeader
        title="AutoClaw 环境配置"
        desc="客户端路径 / 数据目录 / 定时签到；配置全部保存在本机（不落云端）"
        actions={
          <button className="btn-primary" onClick={() => void save()} disabled={saving}>
            {saving ? <Spinner /> : <Save size={15} />}
            保存配置
          </button>
        }
      />

      <div className="grid grid-cols-2 gap-3">
        <div className="card p-4">
          <div className="mb-3 flex items-center justify-between">
            <div className="text-sm font-medium text-slate-700 dark:text-zinc-200">客户端状态</div>
            <Badge tone={env?.running ? 'green' : 'slate'}>{env?.running ? '运行中' : '未运行'}</Badge>
          </div>
          <dl className="space-y-2 text-xs">
            <div className="flex gap-2">
              <dt className="w-20 shrink-0 text-slate-400">安装</dt>
              <dd className="min-w-0 flex-1 break-all text-slate-600 dark:text-zinc-300">
                {env?.installed ? env.exe_path : '未检测到（可在下方手动指定）'}
              </dd>
            </div>
            <div className="flex gap-2">
              <dt className="w-20 shrink-0 text-slate-400">数据目录</dt>
              <dd className="min-w-0 flex-1 break-all text-slate-600 dark:text-zinc-300">{env?.data_dir ?? '未找到'}</dd>
            </div>
            <div className="flex gap-2">
              <dt className="w-20 shrink-0 text-slate-400">账号目录</dt>
              <dd className="min-w-0 flex-1 break-all text-slate-600 dark:text-zinc-300">
                {env?.accounts_dir ?? '—'}（{env?.account_dirs ?? 0} 个）
              </dd>
            </div>
            <div className="flex gap-2">
              <dt className="w-20 shrink-0 text-slate-400">Local State</dt>
              <dd className="min-w-0 flex-1 text-slate-600 dark:text-zinc-300">
                {env?.has_local_state ? <span className="text-emerald-600">已找到（可解密本机凭据）</span> : <span className="text-amber-600">缺失，无法扫描本机账号</span>}
              </dd>
            </div>
          </dl>
          <div className="mt-3 flex flex-wrap gap-2">
            <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => void api.autoclaw.openDataDir().catch((e) => pushToast('error', String(e)))}>
              <FolderOpen size={13} /> 打开数据目录
            </button>
            <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => void api.autoclaw.openApp().catch((e) => pushToast('error', String(e)))}>
              <ExternalLink size={13} /> 启动客户端
            </button>
            <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => void api.autoclaw.openSite('cn')}>
              <ExternalLink size={13} /> 打开官网
            </button>
          </div>
        </div>

        <div className="card p-4">
          <div className="mb-3 flex items-center gap-2 text-sm font-medium text-slate-700 dark:text-zinc-200">
            <Timer size={15} /> 定时签到
          </div>
          <label className="flex items-center justify-between rounded-lg border border-slate-100 px-3 py-2 dark:border-zinc-800">
            <div>
              <div className="text-sm text-slate-700 dark:text-zinc-200">启用定时签到</div>
              <div className="text-xs text-slate-400">关闭后计划任务仍存在，但账号池为空时不会执行</div>
            </div>
            <input
              type="checkbox"
              className="h-4 w-4 accent-zinc-900"
              checked={settings.auto_checkin}
              onChange={(e) => setSettings({ ...settings, auto_checkin: e.target.checked })}
            />
          </label>
          <div className="mt-3 flex items-end gap-2">
            <div className="flex-1">
              <label className="mb-1 block text-xs text-slate-500">每日执行时刻</label>
              <input
                type="time"
                className="input w-full"
                value={settings.checkin_hhmm}
                onChange={(e) => setSettings({ ...settings, checkin_hhmm: e.target.value })}
              />
            </div>
            <button className="btn-ghost" onClick={() => void registerTask()} disabled={registering}>
              {registering ? <Spinner /> : <Bell size={14} />}
              注册系统任务
            </button>
          </div>
          <div className="mt-3 flex items-center justify-between rounded-lg bg-slate-50 px-3 py-2 text-xs dark:bg-zinc-800/60">
            <span className="text-slate-500 dark:text-zinc-400">
              状态：{registered ? <span className="text-emerald-600">已注册 {registeredTime}</span> : '未注册'}
            </span>
            {registered && (
              <button className="text-rose-500 hover:underline" onClick={() => void unregisterTask()} disabled={registering}>
                注销
              </button>
            )}
          </div>
          <p className="mt-2 text-[11px] leading-relaxed text-slate-400">
            计划任务名 <code className="rounded bg-slate-100 px-1 dark:bg-zinc-800">AIWorkAssistant_AutoClawCheckin</code>，
            直调主程序 <code className="rounded bg-slate-100 px-1 dark:bg-zinc-800">--task-run autoclaw-checkin</code>（无黑框）。
          </p>
        </div>
      </div>

      <div className="card mt-4 p-4">
        <div className="mb-3 flex items-center gap-2 text-sm font-medium text-slate-700 dark:text-zinc-200">
          <RefreshCw size={15} /> 路径覆盖（自动探测失败时手填）
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="mb-1 block text-xs text-slate-500">AutoClaw 数据目录（userData）</label>
            <input
              className="input w-full font-mono text-xs"
              placeholder="例如 C:\Users\你\AppData\Roaming\AutoClaw-official"
              value={settings.data_dir}
              onChange={(e) => setSettings({ ...settings, data_dir: e.target.value })}
            />
            <p className="mt-1 text-[11px] text-slate-400">
              含 <code>Local State</code> 与 <code>accounts\</code>；默认识别 AutoClaw-official / AutoClaw / AutoClaw2。
            </p>
          </div>
          <div>
            <label className="mb-1 block text-xs text-slate-500">AutoClaw 主程序路径（可选）</label>
            <input
              className="input w-full font-mono text-xs"
              placeholder="例如 D:\AutoClaw2\AutoClaw2.exe"
              value={settings.app_path}
              onChange={(e) => setSettings({ ...settings, app_path: e.target.value })}
            />
            <p className="mt-1 text-[11px] text-slate-400">用于「启动客户端」；留空则按常见安装位置与进程自动定位。</p>
          </div>
        </div>
      </div>

      <div className="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-4 text-xs leading-relaxed text-amber-800 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200">
        <div className="mb-1 flex items-center gap-1.5 font-medium"><AlertTriangle size={14} /> 使用边界</div>
        <ul className="list-disc space-y-1 pl-5">
          <li>本页只读取本机已登录用户自己的凭据，不提供批量注册、验证码绕过等能力。</li>
          <li>签到接口由官方客户端逆向确认，可能随版本更新失效；失败时界面会明确提示，不会静默重试轰炸。</li>
          <li><strong>账号切换暂未支持</strong>：AutoClaw 客户端以 <code>accounts/&lt;id&gt;</code> 目录管理登录态，换号机制未公开；本应用不会改写其凭据目录，避免破坏客户端登录态。</li>
        </ul>
      </div>

      <div className="mt-3 flex items-center gap-2 text-xs text-slate-400">
        <CheckCircle2 size={13} className="text-emerald-500" />
        配置变更即时生效：保存后「扫描本机」「签到」「积分」都会使用新的目录与区服设置。
      </div>
    </div>
  );
}
