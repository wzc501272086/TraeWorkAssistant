/** AutoClaw 页面共用的小工具与文案（无 JSX，便于各页复用） */

export const REGION_LABEL: Record<string, string> = {
  cn: '国内',
  global: '海外',
};

export function regionLabel(r: string | undefined | null): string {
  if (!r) return '国内';
  return REGION_LABEL[r] ?? r;
}

export function fmtNum(n: number | null | undefined): string {
  if (n == null || Number.isNaN(n)) return '—';
  return new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 0 }).format(n);
}

export function todayStr(): string {
  const d = new Date();
  const mm = String(d.getMonth() + 1).padStart(2, '0');
  const dd = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${mm}-${dd}`;
}

export function shortDate(date: string): string {
  const parts = date.split('-');
  return parts.length === 3 ? `${parts[1]}/${parts[2]}` : date;
}
