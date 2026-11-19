import { useCallback, useEffect, useState } from 'react';
import {
  Activity,
  ArrowRight,
  CalendarCheck,
  Coins,
  PlayCircle,
  RefreshCw,
  Users,
} from 'lucide-react';
import PageHeader from '../../components/PageHeader';
import { Badge, EmptyState, Spinner, StatCard } from '../../components/ui';
import { api } from '../../lib/tauri';
import { useAppStore } from '../../store';
import type { AutoClawDashboard, AutoClawEnvCheck } from '../../types';
import { fmtNum, regionLabel, shortDate, todayStr } from './shared';

/**
 * AutoClaw 概述页：环境状态 + 四张统计卡 + 近 14 天签到趋势 + 上手三步 + 最近记录。
 * 数据全部来自后端聚合（autoclaw_dashboard），单次加载失败只 toast，不阻塞其它卡片。
 */
export default function AutoClawOverview() {
  const pushToast = useAppStore((s) => s.pushToast);
  const setView = useAppStore((s) => s.setView);
  const [env, setEnv] = useState<AutoClawEnvCheck | null>(null);
  const [dash, setDash] = useState<AutoClawDashboard | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(
    async (silent = false) => {
      if (!silent) setLoading(true);
      try {
        const [e, d] = await Promise.all([api.autoclaw.envCheck(), api.autoclaw.dashboard()]);
        setEnv(e);
        setDash(d);
      } catch (err) {
        pushToast('error', `读取 AutoClaw 概览失败：${String(err)}`);
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    // pushToast 由 store 提供，引用稳定
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  useEffect(() => {
    void load();
  }, [load]);

  const refresh = async () => {
    setRefreshing(true);
    await load(true);
  };

  const maxTrend = Math.max(1, ...(dash?.trend ?? []).map((t) => t.success + t.failed));
  const checkedPct = dash && dash.accounts > 0 ? Math.round((dash.checked_today / dash.accounts) * 100) : 0;

  return (
    <div>
      <PageHeader
        title="AutoClaw 概述"
        desc="智谱澳龙桌面 Agent：多账号签到与积分管理（国内 zhipuai.cn / 海外 autoclaw.ai 双区）"
        actions={
          <>
            <button className="btn-ghost" onClick={() => void refresh()} disabled={refreshing}>
              {refreshing ? <Spinner /> : <RefreshCw size={15} />}
              刷新
            </button>
            <button className="btn-ghost" onClick={() => void api.autoclaw.openApp().catch((e) => pushToast('error', String(e)))}>
              <ArrowRight size={15} />
              打开客户端
            </button>
            <button className="btn-primary" onClick={() => setView('autoclaw-checkin')}>
              <PlayCircle size={15} />
              去签到
            </button>
          </>
        }
      />

      {loading ? (
        <div className="flex h-40 items-center justify-center text-slate-400">
          <Spinner className="mr-2" /> 正在读取…
        </div>
      ) : (
        <>
          <div className="grid grid-cols-4 gap-3">
            <StatCard
              label="账号总数"
              value={dash?.accounts ?? 0}
              hint={env?.accounts_dir ? '本机凭据目录已发现' : '尚未发现本机登录态'}
              tone="brand"
            />
            <StatCard
              label="今日已签"
              value={dash?.checked_today ?? 0}
              hint={dash && dash.accounts > 0 ? `覆盖率 ${checkedPct}%` : '先导入账号'}
              tone="green"
            />
            <StatCard
              label="积分余额"
              value={fmtNum(dash?.total_credits ?? 0)}
              hint="各账号钱包合计（最近一次查询）"
              tone="amber"
            />
            <StatCard
              label="今日签到收益"
              value={fmtNum(dash?.today_reward ?? 0)}
              hint="按签到记录汇总（约 400 积分/天/账号）"
              tone="violet"
            />
          </div>

          <div className="mt-4 grid grid-cols-3 gap-3">
            <div className="card col-span-2 p-4">
              <div className="mb-3 flex items-center justify-between">
                <div className="flex items-center gap-2 text-sm font-medium text-slate-700 dark:text-zinc-200">
                  <Activity size={15} /> 近 14 天签到
                </div>
                <div className="flex items-center gap-3 text-xs text-slate-400">
                  <span className="flex items-center gap-1">
                    <i className="inline-block h-2 w-2 rounded-sm bg-emerald-500" /> 成功/已签
                  </span>
                  <span className="flex items-center gap-1">
                    <i className="inline-block h-2 w-2 rounded-sm bg-rose-400" /> 失败
                  </span>
                </div>
              </div>
              <div className="flex h-36 items-end gap-1.5">
                {(dash?.trend ?? []).map((t) => {
                  const total = t.success + t.failed;
                  const h = Math.max(4, Math.round((total / maxTrend) * 100));
                  return (
                    <div key={t.date} className="flex flex-1 flex-col items-center gap-1" title={`${t.date} 成功 ${t.success} / 失败 ${t.failed}`}>
                      <div className="flex h-28 w-full flex-col justify-end overflow-hidden rounded-t bg-slate-100 dark:bg-zinc-800">
                        <div className="w-full bg-emerald-500" style={{ height: `${(t.success / Math.max(1, total)) * h}%` }} />
                        <div className="w-full bg-rose-400" style={{ height: `${(t.failed / Math.max(1, total)) * h}%` }} />
                      </div>
                      <div className="text-[10px] text-slate-400">{shortDate(t.date)}</div>
                    </div>
                  );
                })}
              </div>
            </div>

            <div className="card p-4">
              <div className="mb-3 text-sm font-medium text-slate-700 dark:text-zinc-200">上手三步</div>
              <ol className="space-y-3 text-sm">
                <li className="flex gap-2">
                  <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-slate-900 text-[11px] text-white dark:bg-zinc-100 dark:text-zinc-900">1</span>
                  <div>
                    <div className="text-slate-700 dark:text-zinc-200">安装并登录 AutoClaw</div>
                    <div className="text-xs text-slate-400">{env?.installed ? '已检测到客户端' : '未检测到，可在环境配置页指定路径'}</div>
                  </div>
                </li>
                <li className="flex gap-2">
                  <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-slate-900 text-[11px] text-white dark:bg-zinc-100 dark:text-zinc-900">2</span>
                  <div>
                    <div className="text-slate-700 dark:text-zinc-200">导入账号</div>
                    <div className="text-xs text-slate-400">扫描本机凭据，或粘贴 accessToken</div>
                  </div>
                </li>
                <li className="flex gap-2">
                  <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-slate-900 text-[11px] text-white dark:bg-zinc-100 dark:text-zinc-900">3</span>
                  <div>
                    <div className="text-slate-700 dark:text-zinc-200">开启定时签到</div>
                    <div className="text-xs text-slate-400">默认 09:40，可在环境配置页调整</div>
                  </div>
                </li>
              </ol>
              <div className="mt-4 flex flex-wrap gap-2">
                <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => setView('autoclaw-accounts')}>
                  <Users size={13} /> 账号管理
                </button>
                <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => setView('autoclaw-credits')}>
                  <Coins size={13} /> 积分看板
                </button>
                <button className="btn-ghost !px-2 !py-1 text-xs" onClick={() => setView('autoclaw-settings')}>
                  <CalendarCheck size={13} /> 环境配置
                </button>
              </div>
            </div>
          </div>

          <div className="card mt-4 p-4">
            <div className="mb-3 text-sm font-medium text-slate-700 dark:text-zinc-200">最近签到记录</div>
            {(dash?.recent ?? []).length === 0 ? (
              <EmptyState title="暂无签到记录" hint="点击右上角「去签到」执行一轮，记录会落库并在此展示" />
            ) : (
              <div className="divide-y divide-slate-100 dark:divide-zinc-800">
                {(dash?.recent ?? []).map((r, i) => (
                  <div key={`${r.date}-${r.user_id}-${i}`} className="flex items-center gap-3 py-2 text-sm">
                    <span className="w-24 shrink-0 text-xs text-slate-400">{r.date} {r.time}</span>
                    <span className="min-w-0 flex-1 truncate text-slate-700 dark:text-zinc-200">{r.name || r.user_id}</span>
                    <Badge tone="slate">{regionLabel(r.region)}</Badge>
                    <Badge tone={r.status === 'success' ? 'green' : r.status === 'already' ? 'blue' : 'red'}>
                      {r.status === 'success' ? '签到成功' : r.status === 'already' ? '今日已签' : '失败'}
                    </Badge>
                    <span className="w-16 text-right text-xs tabular-nums text-amber-600">
                      {r.reward != null ? `+${fmtNum(r.reward)}` : ''}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </div>

          {dash && dash.accounts === 0 && (
            <div className="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-3 text-sm text-amber-800 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200">
              账号池为空：先到「账号管理」扫描本机 AutoClaw 登录态，或手工粘贴 accessToken。
              <span className="ml-2 text-xs opacity-70">今日 {todayStr()}</span>
            </div>
          )}
        </>
      )}
    </div>
  );
}
