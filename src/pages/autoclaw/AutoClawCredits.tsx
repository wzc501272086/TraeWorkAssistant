import { useCallback, useEffect, useMemo, useState } from 'react';
import { Coins, RefreshCw, Wallet } from 'lucide-react';
import PageHeader from '../../components/PageHeader';
import { Badge, EmptyState, Spinner, StatCard } from '../../components/ui';
import { api } from '../../lib/tauri';
import { useAppStore } from '../../store';
import type { AutoClawCreditsResult, AutoClawCreditsSnapshot } from '../../types';
import { fmtNum, regionLabel, shortDate } from './shared';

const WALLET_LABEL: Record<string, string> = {
  reward: 'Reward 奖励积分',
  daily: 'Daily 每日积分',
  subscription: '订阅积分',
  fuel_pack: '加油包',
  other: '其他',
};

export default function AutoClawCredits() {
  const pushToast = useAppStore((s) => s.pushToast);
  const [data, setData] = useState<AutoClawCreditsResult | null>(null);
  const [history, setHistory] = useState<AutoClawCreditsSnapshot[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  const load = useCallback(async (fresh = false) => {
    if (fresh) setRefreshing(true);
    else setLoading(true);
    try {
      const [c, h] = await Promise.all([
        api.autoclaw.creditsFetch(undefined, fresh),
        api.autoclaw.creditsHistory(),
      ]);
      setData(c);
      setHistory(h);
    } catch (err) {
      pushToast('error', `积分查询失败：${String(err)}`);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void load(false);
  }, [load]);

  const maxHistory = useMemo(
    () => Math.max(1, ...history.map((h) => h.total)),
    [history],
  );

  const okCount = data?.accounts.filter((a) => a.ok).length ?? 0;

  return (
    <div>
      <PageHeader
        title="AutoClaw 积分看板"
        desc="钱包接口 /agent-assetmgr/api/v2/wallets（600s 缓存；失败不影响已有余额展示）"
        actions={
          <button className="btn-ghost" onClick={() => void load(true)} disabled={refreshing}>
            {refreshing ? <Spinner /> : <RefreshCw size={15} />}
            刷新积分
          </button>
        }
      />

      {loading ? (
        <div className="flex h-40 items-center justify-center text-slate-400">
          <Spinner className="mr-2" /> 正在查询钱包…
        </div>
      ) : (
        <>
          <div className="grid grid-cols-4 gap-3">
            <StatCard label="总余额" value={fmtNum(data?.total_balance ?? 0)} tone="amber" hint="全部账号可用积分合计" />
            <StatCard label="账号数" value={data?.accounts.length ?? 0} tone="brand" hint={`${okCount} 个查询成功`} />
            <StatCard label="缓存状态" value={data?.cached ? '缓存' : '实时'} tone="blue" hint={data?.fetched_at ?? '—'} />
            <StatCard
              label="失败账号"
              value={(data?.accounts.length ?? 0) - okCount}
              tone={(data?.accounts.length ?? 0) - okCount > 0 ? 'red' : 'slate'}
              hint="失败多为登录态失效"
            />
          </div>

          <div className="mt-4 grid grid-cols-3 gap-3">
            <div className="card col-span-2 p-4">
              <div className="mb-3 flex items-center gap-2 text-sm font-medium text-slate-700 dark:text-zinc-200">
                <Coins size={15} /> 账号钱包
              </div>
              {data?.accounts.length === 0 ? (
                <EmptyState title="没有可查询的账号" hint="先到「账号管理」导入账号" />
              ) : (
                <div className="space-y-2">
                  {data?.accounts.map((a) => (
                    <div
                      key={a.user_id}
                      className="rounded-lg border border-slate-100 p-3 dark:border-zinc-800"
                    >
                      <div className="flex items-center gap-2">
                        <span className="min-w-0 flex-1 truncate text-sm font-medium text-slate-700 dark:text-zinc-100">
                          {a.name || a.user_id}
                        </span>
                        <Badge tone={a.region === 'global' ? 'violet' : 'blue'}>{regionLabel(a.region)}</Badge>
                        {a.cached && <Badge tone="slate">缓存</Badge>}
                        <span className="text-sm font-semibold tabular-nums text-amber-600">
                          {a.ok ? fmtNum(a.total_balance) : '—'}
                        </span>
                      </div>
                      {a.ok ? (
                        <div className="mt-2 flex flex-wrap gap-1.5">
                          {a.wallets.length === 0 ? (
                            <span className="text-xs text-slate-400">钱包明细为空（接口未返回 wallets）</span>
                          ) : (
                            a.wallets.map((w, i) => (
                              <span
                                key={`${w.kind}-${i}`}
                                className="rounded-md bg-slate-100 px-2 py-0.5 text-xs text-slate-600 dark:bg-zinc-800 dark:text-zinc-300"
                              >
                                {WALLET_LABEL[w.kind] ?? w.kind}：{fmtNum(w.balance)}
                              </span>
                            ))
                          )}
                        </div>
                      ) : (
                        <div className="mt-1 text-xs text-rose-500">{a.message || '查询失败'}</div>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="card p-4">
              <div className="mb-3 flex items-center gap-2 text-sm font-medium text-slate-700 dark:text-zinc-200">
                <Wallet size={15} /> 余额趋势（每日快照）
              </div>
              {history.length === 0 ? (
                <div className="py-8 text-center text-xs text-slate-400">尚无历史快照：刷新一次积分后开始记录</div>
              ) : (
                <div className="flex h-40 items-end gap-1">
                  {history.slice(-14).map((h) => (
                    <div key={h.date} className="flex flex-1 flex-col items-center gap-1" title={`${h.date}：${fmtNum(h.total)}`}>
                      <div
                        className="w-full rounded-t bg-gradient-to-t from-amber-400 to-amber-300"
                        style={{ height: `${Math.max(4, (h.total / maxHistory) * 100)}%` }}
                      />
                      <span className="text-[10px] text-slate-400">{shortDate(h.date)}</span>
                    </div>
                  ))}
                </div>
              )}
              <div className="mt-3 text-xs leading-relaxed text-slate-400">
                每日首次查询会落一条快照（保留 365 天），用于观察积分消耗与签到补充。
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
