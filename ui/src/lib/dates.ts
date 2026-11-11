// Local-time date helpers. Dates travel as "YYYY-MM-DD", moments as "YYYY-MM-DDTHH:MM:SS".

export const pad = (n: number) => String(n).padStart(2, "0");

export function fmtDate(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function fmtDateTime(d: Date): string {
  return `${fmtDate(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}:00`;
}

export function parseDate(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y, m - 1, d);
}

export function todayStr(): string {
  return fmtDate(new Date());
}

export function addDays(s: string, n: number): string {
  const d = parseDate(s);
  d.setDate(d.getDate() + n);
  return fmtDate(d);
}

export function diffDays(a: string, b: string): number {
  return Math.round((parseDate(a).getTime() - parseDate(b).getTime()) / 86400000);
}

const WEEK = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

export function weekday(s: string): string {
  return WEEK[parseDate(s).getDay()];
}

export function monthDay(s: string): string {
  const d = parseDate(s);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

/** "今天" / "明天" / "昨天" … or "11月14日" for anything further away. */
export function relativeDay(s: string, today: string): string {
  const names: Record<number, string> = { 0: "今天", 1: "明天", 2: "后天", [-1]: "昨天", [-2]: "前天" };
  return names[diffDays(s, today)] ?? monthDay(s);
}

/** "HH:MM" out of a stored moment. */
export function timeOf(moment: string | null): string {
  return moment ? moment.slice(11, 16) : "";
}

export function monthOf(s: string): string {
  return s.slice(0, 7);
}

export function addMonths(ym: string, n: number): string {
  const [y, m] = ym.split("-").map(Number);
  const d = new Date(y, m - 1 + n, 1);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
}

export function monthRange(ym: string): { from: string; to: string } {
  const [y, m] = ym.split("-").map(Number);
  return { from: `${ym}-01`, to: fmtDate(new Date(y, m, 0)) };
}

/** Six weeks of dates covering the month, weeks starting on Monday. */
export function monthGrid(ym: string): string[] {
  const first = parseDate(`${ym}-01`);
  const lead = (first.getDay() + 6) % 7;
  const start = addDays(`${ym}-01`, -lead);
  return Array.from({ length: 42 }, (_, i) => addDays(start, i));
}

/** Human text for a recurrence rule: daily / weekdays / weekly:1,3,5 / monthly:15. */
export function ruleText(rule: string): string {
  if (rule === "daily") return "每天";
  if (rule === "weekdays") return "工作日";
  if (rule.startsWith("weekly:")) {
    const names = ["", "一", "二", "三", "四", "五", "六", "日"];
    const days = rule
      .slice(7)
      .split(",")
      .map((n) => names[Number(n)] ?? "")
      .join("、");
    return `每周${days}`;
  }
  if (rule.startsWith("monthly:")) return `每月 ${rule.slice(8)} 日`;
  return rule;
}
