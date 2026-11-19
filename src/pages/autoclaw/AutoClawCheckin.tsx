import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { CheckCircle2, PlayCircle, RotateCcw, XCircle } from 'lucide-react';
import PageHeader from '../../components/PageHeader';
import { Badge, EmptyState, Spinner, StatCard } from '../../components/ui';
import { api } from '../../lib/tauri';
import { useAppStore } from '../../store';
import type { AutoClawCheckinRecord } from '../../types';
import { fmtNum, regionLabel } from './shared';

type Line = {
  type: string;
  stage?: string;
  status?: string;
  message?: string;
  uid?: string;
  name?: string;
  time?: string;
  reward?: number;
  ok?: boolean;
  total?: number;
  ok_count?: number;
  already?: number;
  failed?: number;
};

type Done = { ok: boolean; total: number; ok_count: number; already: number; failed: number } | null;

/**
 * AutoClaw 一键签到：UI 触发后台线程，NDJSON 逐行回流（autoclaw-checkin-progress），
 * 结束后 exit 事件复位运行态。与 CLI 计划任务共用同一轮次互斥（后端 ROUND_BUSY）。
 */
export default function AutoClawCheckin() {
  const pushToast = useAppStore((s) => s.pushToast);
  const [running, setRunning] = useState(false);
  const [lines, setLines] = useState<Line[]>([]);
  const [done, setDone] = useState<Done>(null);
  const [region, setRegion] = useState<'all' | 'cn' | 'global'>('all');
  const [records, setRecords] = useState<AutoClawCheckinRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const logRef = useRef<HTMLDivElement | null>(null);

  const loadRecords = useCallback(async () => {
    setLoading(true);
    try {
      setRecords(await api.autoclaw.checkinResults(30));
    } catch (err) {
      pushToast('error', `读取签到记录失败：${String(err)}`);
    } finally {
      setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void loadRecords();
  }, [loadRecords]);

  // 事件订阅：payload 是 NDJSON 字符串（与 wb/qoder 管线同款，传对象会破坏 parse）
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    void listen<string>('autoclaw-checkin-progress', (ev) => {
      try {
        const line = JSON.parse(ev.payload) as Line;
        if (line.type === 'start') {
          setLines([]);
          setDone(null);
          setRunning(true);
          return;
        }
        if (line.type === 'exit') {
          setRunning(false);
          void loadRecords();
          return;
        }
        if (line.type === 'done') {
          setDone({
            ok: Boolean(line.ok),
            total: Number(line.total ?? 0),
            ok_count: Number(line.ok_count ?? 0),
            already: Number(line.already ?? 0),
            failed: Number(line.failed ?? 0),
          });
          setRunning(false);
          return;
        }
        setLines((prev) => [...prev, line]);
      } catch {
        /* 非 JSON 行忽略 */
      }
    }).then((fn) => {
      if (alive) unlisten = fn;
      else fn();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [loadRecords]);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [lines]);

  const start = async () => {
    if (running) return;
    try {
      setLines([]);
      setDone(null);
      setRunning(true);
      await api.autoclaw.checkinStart({ regions: region === 'all' ? [] : [region] });
    } catch (err) {
      setRunning(false);
      pushToast('error', String(err));
    }
  };

  const today = new Date().toISOString().slice(0, 10);
  const todayRows = records.filter((r) => r.date === today);
  const todayOk = todayRows.filter((r) => r.status === 'success' || r.status === 'already').length;
  const todayReward = todayRows.reduce((s, r) => s + (r.reward ?? 0), 0);

  return (
    <div>
      <PageHeader
        title="AutoClaw 一键签到"
        desc="调用 daily_signin 任务（服务端幂等：重复执行只返回「今日已签」，无副作用）"
        actions={
          <>
            <div className="flex items-center gap-1 rounded-lg bg-slate-100 p-1 dark:bg-zinc-800">
              {(['all', 'cn', 'global'] as const).map((r) => (
                <button
                  key={r}
                  onClick={() => setRegion(r)}
                  className={
                    region === r
                      ? 'rounded-md bg-white px-2.5 py-1 text-xs font-medium text-zinc-900 shadow-sm dark:bg-zinc-700 dark:text-zinc-100'
                      : 'rounded-md px-2.5 py-1 text-xs text-slate-500 hover:text-slate-700 dark:text-zinc-400'
                  }
                >
                  {r === 'all' ? '全部区服' : regionLabel(r)}
                </button>
              ))}
            </div>
            <button className="btn-ghost" onClick={() => void loadRecords()} disabled={loading}>
              {loading ? <Spinner /> : <RotateCcw size={15} />}
              刷新记录
            </button>
            <button className="btn-primary" onClick={() => void start()} disabled={running}>
              {running ? <Spinner /> : <PlayCircle size={15} />}
              {running ? '签到中…' : '开始签到'}
            </button>
          </>
        }
      />

      <div className="grid grid-cols-4 gap-3">
        <StatCard label="今日已签" value={todayOk} tone="green" hint="成功 + 今日已签（去重按账号）" />
        <StatCard label="今日收益" value={fmtNum(todayReward)} tone="amber" hint="本次记录中的 reward_points 合计" />
        <StatCard label="本轮成功" value={done?.ok_count ?? 0} tone="blue" hint={done ? `共 ${done.total} 个账号` : '尚未执行'} />
        <StatCard label="本轮失败" value={done?.failed ?? 0} tone={done && done.failed > 0 ? 'red' : 'slate'} hint={done?.already ? `幂等已签 ${done.already}` : '失败多为凭证失效'} />
      </div>

      <div className="mt-4 grid grid-cols-5 gap-3">
        <div className="card col-span-3 flex h-[22rem] flex-col p-4">
          <div className="mb-2 flex items-center justify-between">
            <div className="text-sm font-medium text-slate-700 dark:text-zinc-200">执行日志</div>
            {running && <Badge tone="blue">运行中</Badge>}
          </div>
          <div ref={logRef} className="min-h-0 flex-1 overflow-y-auto rounded-lg bg-slate-950 p-3 font-mono text-xs text-zinc-200">
            {lines.length === 0 && !running ? (
              <div className="text-zinc-500">点击「开始签到」后，逐账号进度会实时输出到这里。</div>
            ) : (
              lines.map((l, i) => (
                <div key={i} className="flex gap-2 py-0.5">
                  <span className="shrink-0 text-zinc-500">{l.time ?? ''}</span>
                  <span
                    className={
                      l.status === 'ok'
                        ? 'text-emerald-400'
                        : l.status === 'already'
                        ? 'text-sky-400'
                        : l.status === 'fail'
                        ? 'text-rose-400'
                        : 'text-zinc-300'
                    }
                  >
                    {l.message ?? JSON.stringify(l)}
                  </span>
                </div>
              ))
            )}
            {running && (
              <div className="mt-1 flex items-center gap-2 text-zinc-400">
                <Spinner /> 执行中…
              </div>
            )}
          </div>
        </div>

        <div className="card col-span-2 p-4">
          <div className="mb-2 text-sm font-medium text-slate-700 dark:text-zinc-200">签到说明</div>
          <ul className="space-y-2 text-xs leading-relaxed text-slate-500 dark:text-zinc-400">
            <li className="flex gap-1.5"><CheckCircle2 size={13} className="mt-0.5 shrink-0 text-emerald-500" />签到接口：<code>POST /autoclaw-proxy/proxy/autoclaw-task-complete</code>，body <code>{'{ "task_id": "daily_signin" }'}</code></li>
            <li className="flex gap-1.5"><CheckCircle2 size={13} className="mt-0.5 shrink-0 text-emerald-500" />每日奖励约 400 积分，服务端幂等：重复执行不会重复发放</li>
            <li className="flex gap-1.5"><CheckCircle2 size={13} className="mt-0.5 shrink-0 text-emerald-500" />国内 / 海外账号分别调用各自区服基址，混跑一轮即可全覆盖</li>
            <li className="flex gap-1.5"><XCircle size={13} className="mt-0.5 shrink-0 text-rose-500" />出现「登录态失效」时：重新打开 AutoClaw 登录一次，再回账号页「扫描本机」刷新凭据</li>
          </ul>
        </div>
      </div>

      <div className="card mt-4 overflow-hidden">
        <div className="flex items-center justify-between border-b border-slate-100 px-4 py-3 dark:border-zinc-800">
          <div className="text-sm font-medium text-slate-700 dark:text-zinc-200">签到记录（近 30 天）</div>
          <span className="text-xs text-slate-400">{records.length} 条</span>
        </div>
        {records.length === 0 ? (
          <div className="p-6">
            <EmptyState title="暂无签到记录" hint="执行一轮签到后自动落库（保留 90 天，默认展示 30 天）" />
          </div>
        ) : (
          <div className="max-h-[26rem] overflow-y-auto">
            <table className="w-full text-sm">
              <thead className="sticky top-0 bg-white text-left text-xs text-slate-400 dark:bg-zinc-900">
                <tr className="border-b border-slate-100 dark:border-zinc-800">
                  <th className="px-4 py-2 font-medium">时间</th>
                  <th className="px-3 py-2 font-medium">账号</th>
                  <th className="px-3 py-2 font-medium">区服</th>
                  <th className="px-3 py-2 font-medium">结果</th>
                  <th className="px-3 py-2 font-medium">说明</th>
                  <th className="px-3 py-2 text-right font-medium">奖励</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 dark:divide-zinc-800">
                {records.map((r, i) => (
                  <tr key={`${r.date}-${r.time}-${r.user_id}-${i}`}>
                    <td className="whitespace-nowrap px-4 py-2 text-xs text-slate-400">{r.date} {r.time}</td>
                    <td className="px-3 py-2 text-slate-700 dark:text-zinc-200">{r.name || r.user_id}</td>
                    <td className="px-3 py-2"><Badge tone={r.region === 'global' ? 'violet' : 'blue'}>{regionLabel(r.region)}</Badge></td>
                    <td className="px-3 py-2">
                      <Badge tone={r.status === 'success' ? 'green' : r.status === 'already' ? 'blue' : 'red'}>
                        {r.status === 'success' ? '成功' : r.status === 'already' ? '已签' : '失败'}
                      </Badge>
                    </td>
                    <td className="max-w-[22rem] truncate px-3 py-2 text-xs text-slate-500" title={r.message}>{r.message}</td>
                    <td className="px-3 py-2 text-right text-xs tabular-nums text-amber-600">{r.reward != null ? `+${fmtNum(r.reward)}` : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
