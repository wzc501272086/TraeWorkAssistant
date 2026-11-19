import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Coins,
  Pencil,
  RefreshCw,
  ScanSearch,
  Trash2,
  Upload,
  Users,
} from 'lucide-react';
import PageHeader from '../../components/PageHeader';
import { Badge, EmptyState, Modal, Spinner } from '../../components/ui';
import { api } from '../../lib/tauri';
import { useAppStore } from '../../store';
import type { AutoClawAccountView, AutoClawScanResult } from '../../types';
import { fmtNum, regionLabel } from './shared';

type EditState = { id: string; name: string; note: string; region: string } | null;

export default function AutoClawAccounts() {
  const pushToast = useAppStore((s) => s.pushToast);
  const [accounts, setAccounts] = useState<AutoClawAccountView[]>([]);
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [creditsLoading, setCreditsLoading] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [token, setToken] = useState('');
  const [region, setRegion] = useState('cn');
  const [name, setName] = useState('');
  const [importing, setImporting] = useState(false);
  const [edit, setEdit] = useState<EditState>(null);
  const [scanResult, setScanResult] = useState<AutoClawScanResult | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setAccounts(await api.autoclaw.accountsList());
    } catch (err) {
      pushToast('error', `读取账号池失败：${String(err)}`);
    } finally {
      setLoading(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const stats = useMemo(() => {
    const totalCredits = accounts.reduce((s, a) => s + (a.credits_balance ?? 0), 0);
    return {
      total: accounts.length,
      cn: accounts.filter((a) => a.region !== 'global').length,
      global: accounts.filter((a) => a.region === 'global').length,
      totalCredits,
      needRelogin: accounts.filter((a) => a.needs_relogin).length,
    };
  }, [accounts]);

  const doScan = async () => {
    setScanning(true);
    try {
      const r = await api.autoclaw.scanLocal();
      setScanResult(r);
      await load();
      if (r.found > 0) pushToast('success', `扫描完成：新增 ${r.imported} / 更新 ${r.updated}`);
      else pushToast('warn', r.errors[0] ?? '未发现本机 AutoClaw 登录态');
    } catch (err) {
      pushToast('error', `扫描失败：${String(err)}`);
    } finally {
      setScanning(false);
    }
  };

  const doImport = async () => {
    if (!token.trim()) {
      pushToast('warn', '请粘贴 accessToken');
      return;
    }
    setImporting(true);
    try {
      await api.autoclaw.importToken(token.trim(), region, name.trim() || undefined);
      pushToast('success', '账号已入池');
      setImportOpen(false);
      setToken('');
      setName('');
      await load();
    } catch (err) {
      pushToast('error', String(err));
    } finally {
      setImporting(false);
    }
  };

  const refreshCredits = async () => {
    setCreditsLoading(true);
    try {
      const r = await api.autoclaw.creditsFetch(undefined, true);
      await load();
      pushToast(r.ok ? 'success' : 'warn', r.ok ? `积分已刷新，合计 ${fmtNum(r.total_balance)}` : '积分查询失败，请检查凭证');
    } catch (err) {
      pushToast('error', String(err));
    } finally {
      setCreditsLoading(false);
    }
  };

  const remove = async (a: AutoClawAccountView) => {
    if (!window.confirm(`确定删除账号「${a.nickname || a.uid}」？仅移除本应用内的凭据记录，不影响 AutoClaw 客户端。`)) return;
    try {
      await api.autoclaw.accountRemove(a.id);
      pushToast('success', '已删除');
      await load();
    } catch (err) {
      pushToast('error', String(err));
    }
  };

  const saveEdit = async () => {
    if (!edit) return;
    try {
      await api.autoclaw.accountUpdate(edit.id, {
        name: edit.name,
        note: edit.note,
        region: edit.region,
      });
      pushToast('success', '已保存');
      setEdit(null);
      await load();
    } catch (err) {
      pushToast('error', String(err));
    }
  };

  return (
    <div>
      <PageHeader
        title="AutoClaw 账号管理"
        desc={`${stats.total} 个账号（国内 ${stats.cn} / 海外 ${stats.global}）· 积分合计 ${fmtNum(stats.totalCredits)}`}
        actions={
          <>
            <button className="btn-ghost" onClick={() => void doScan()} disabled={scanning}>
              {scanning ? <Spinner /> : <ScanSearch size={15} />}
              扫描本机
            </button>
            <button className="btn-ghost" onClick={() => void refreshCredits()} disabled={creditsLoading}>
              {creditsLoading ? <Spinner /> : <Coins size={15} />}
              刷新积分
            </button>
            <button className="btn-primary" onClick={() => setImportOpen(true)}>
              <Upload size={15} />
              导入 Token
            </button>
          </>
        }
      />

      <div className="mb-3 rounded-xl border border-slate-200 bg-white/60 p-3 text-xs leading-relaxed text-slate-500 dark:border-zinc-800 dark:bg-zinc-900/40 dark:text-zinc-400">
        <strong className="text-slate-700 dark:text-zinc-200">账号来源：</strong>
        「扫描本机」读取 AutoClaw 客户端 <code className="rounded bg-slate-100 px-1 dark:bg-zinc-800">accounts/&lt;id&gt;/account-credentials.enc</code>
        （Chromium v10，DPAPI + AES-256-GCM，仅当前 Windows 用户可解），无需手工复制 token。
        若同一台机器登录过多个账号，先在 AutoClaw 里逐个登录一次再扫描即可全部入池。
      </div>

      {scanResult && scanResult.errors.length > 0 && (
        <div className="mb-3 space-y-1 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-200">
          {scanResult.errors.map((e, i) => (
            <div key={i}>· {e}</div>
          ))}
        </div>
      )}

      {stats.needRelogin > 0 && (
        <div className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700 dark:border-rose-500/30 dark:bg-rose-500/10 dark:text-rose-300">
          {stats.needRelogin} 个账号登录态失效：请重新打开 AutoClaw 登录，再点「扫描本机」更新凭据。
        </div>
      )}

      <div className="card overflow-hidden">
        {loading ? (
          <div className="flex h-40 items-center justify-center text-slate-400">
            <Spinner className="mr-2" /> 读取账号池…
          </div>
        ) : accounts.length === 0 ? (
          <div className="p-6">
            <EmptyState
              icon={<Users size={22} />}
              title="账号池为空"
              hint="确认 AutoClaw 已登录后点「扫描本机」；或在环境配置页核对数据目录后重试"
            />
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-slate-100 text-left text-xs text-slate-400 dark:border-zinc-800">
                <th className="px-4 py-2.5 font-medium">账号</th>
                <th className="px-3 py-2.5 font-medium">区服</th>
                <th className="px-3 py-2.5 font-medium">来源</th>
                <th className="px-3 py-2.5 font-medium">积分</th>
                <th className="px-3 py-2.5 font-medium">最近签到</th>
                <th className="px-3 py-2.5 font-medium">凭证</th>
                <th className="px-3 py-2.5 text-right font-medium">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100 dark:divide-zinc-800">
              {accounts.map((a) => (
                <tr key={a.id} className="hover:bg-slate-50/70 dark:hover:bg-zinc-900/40">
                  <td className="px-4 py-2.5">
                    <div className="font-medium text-slate-700 dark:text-zinc-100">{a.nickname || '未命名'}</div>
                    <div className="text-xs text-slate-400">
                      {a.token_hint} {a.note ? `· ${a.note}` : ''}
                    </div>
                  </td>
                  <td className="px-3 py-2.5">
                    <Badge tone={a.region === 'global' ? 'violet' : 'blue'}>{regionLabel(a.region)}</Badge>
                  </td>
                  <td className="px-3 py-2.5">
                    <Badge tone={a.credential_source === 'local_scan' ? 'green' : 'slate'}>
                      {a.credential_source === 'local_scan' ? '本机扫描' : 'Token 导入'}
                    </Badge>
                  </td>
                  <td className="px-3 py-2.5 tabular-nums text-amber-600">
                    {a.credits_balance != null ? fmtNum(a.credits_balance) : '—'}
                  </td>
                  <td className="px-3 py-2.5">
                    {a.last_checkin_date ? (
                      <div className="text-xs">
                        <span className="text-slate-500 dark:text-zinc-400">{a.last_checkin_date}</span>
                        <Badge
                          className="ml-1"
                          tone={
                            a.last_checkin_status === 'success'
                              ? 'green'
                              : a.last_checkin_status === 'already'
                              ? 'blue'
                              : 'red'
                          }
                        >
                          {a.last_checkin_status === 'success' ? '成功' : a.last_checkin_status === 'already' ? '已签' : '失败'}
                        </Badge>
                      </div>
                    ) : (
                      <span className="text-xs text-slate-400">未签到</span>
                    )}
                  </td>
                  <td className="px-3 py-2.5">
                    {a.needs_relogin ? (
                      <Badge tone="red" title={a.relogin_reason}>需重新登录</Badge>
                    ) : a.has_credential ? (
                      <Badge tone="green">可用</Badge>
                    ) : (
                      <Badge tone="red">无凭证</Badge>
                    )}
                  </td>
                  <td className="px-3 py-2.5">
                    <div className="flex justify-end gap-1">
                      <button
                        className="btn-ghost h-7 w-7 !p-0"
                        title="编辑名称 / 备注 / 区服"
                        onClick={() => setEdit({ id: a.id, name: a.nickname, note: a.note, region: a.region })}
                      >
                        <Pencil size={14} />
                      </button>
                      <button
                        className="btn-ghost h-7 w-7 !p-0 text-rose-500"
                        title="删除账号"
                        onClick={() => void remove(a)}
                      >
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <Modal
        open={importOpen}
        onClose={() => setImportOpen(false)}
        title="导入 AutoClaw 账号"
        footer={
          <>
            <button className="btn-ghost" onClick={() => setImportOpen(false)}>取消</button>
            <button className="btn-primary" onClick={() => void doImport()} disabled={importing}>
              {importing ? <Spinner /> : <Upload size={14} />}
              导入
            </button>
          </>
        }
      >
        <div className="space-y-3">
          <div>
            <label className="mb-1 block text-xs text-slate-500">accessToken</label>
            <textarea
              className="input h-24 w-full resize-none font-mono text-xs"
              placeholder="粘贴 accessToken（JWT，通常以 eyJ 开头）"
              value={token}
              onChange={(e) => setToken(e.target.value)}
            />
            <p className="mt-1 text-xs text-slate-400">
              获取方式：AutoClaw 客户端登录后由「扫描本机」自动读取；也可从调试工具复制请求头 Authorization。
            </p>
          </div>
          <div className="flex gap-1.5">
            {['cn', 'global'].map((r) => (
              <button
                key={r}
                className={r === region ? 'btn-primary !px-3 !py-1 text-xs' : 'btn-ghost !px-3 !py-1 text-xs'}
                onClick={() => setRegion(r)}
              >
                {regionLabel(r)}
              </button>
            ))}
          </div>
          <div>
            <label className="mb-1 block text-xs text-slate-500">备注名（可选）</label>
            <input className="input w-full" value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：主号" />
          </div>
        </div>
      </Modal>

      <Modal
        open={edit != null}
        onClose={() => setEdit(null)}
        title="编辑账号信息"
        footer={
          <>
            <button className="btn-ghost" onClick={() => setEdit(null)}>取消</button>
            <button className="btn-primary" onClick={() => void saveEdit()}>
              <RefreshCw size={14} /> 保存
            </button>
          </>
        }
      >
        {edit && (
          <div className="space-y-3">
            <div>
              <label className="mb-1 block text-xs text-slate-500">名称</label>
              <input className="input w-full" value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} />
            </div>
            <div>
              <label className="mb-1 block text-xs text-slate-500">备注</label>
              <input className="input w-full" value={edit.note} onChange={(e) => setEdit({ ...edit, note: e.target.value })} />
            </div>
            <div>
              <label className="mb-1 block text-xs text-slate-500">区服</label>
              <div className="flex gap-1.5">
                {['cn', 'global'].map((r) => (
                  <button
                    key={r}
                    className={r === edit.region ? 'btn-primary !px-3 !py-1 text-xs' : 'btn-ghost !px-3 !py-1 text-xs'}
                    onClick={() => setEdit({ ...edit, region: r })}
                  >
                    {regionLabel(r)}
                  </button>
                ))}
              </div>
            </div>
          </div>
        )}
      </Modal>
    </div>
  );
}
