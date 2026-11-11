// Turns one typed line such as "明天下午3点 回访客户" into a title, a date and a reminder time.
// Only unambiguous expressions are recognised; anything else stays in the title untouched.

import { addDays, fmtDate, fmtDateTime, pad, relativeDay, weekday } from "./dates.ts";

export interface QuickParsed {
  title: string;
  date: string;
  remind_at: string | null;
  /** Short confirmation of what was understood, empty when nothing was. */
  hint: string;
}

const WEEK_INDEX: Record<string, number> = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 日: 7, 天: 7 };

/**
 * @param baseDate the day the user is looking at; used when the text names no day.
 * @param now      current local time.
 */
export function parseQuick(input: string, baseDate: string, now: Date): QuickParsed {
  let text = input;
  const today = fmtDate(now);
  let date: string | null = null;
  let time: { h: number; m: number } | null = null;
  let remind: Date | null = null;

  const take = (re: RegExp, fn: (m: RegExpMatchArray) => boolean | void) => {
    const m = text.match(re);
    if (m && fn(m) !== false) text = text.replace(m[0], " ");
  };

  // "30分钟后" / "2小时后"
  take(/(\d{1,3})\s*(分钟|小时)后/, (m) => {
    const minutes = Number(m[1]) * (m[2] === "小时" ? 60 : 1);
    remind = new Date(now.getTime() + minutes * 60000);
    date = fmtDate(remind);
  });

  if (!remind) {
    take(/大后天|后天|明天|明早|今天|今晚|今早/, (m) => {
      const offsets: Record<string, number> = { 今天: 0, 今晚: 0, 今早: 0, 明天: 1, 明早: 1, 后天: 2, 大后天: 3 };
      date = addDays(today, offsets[m[0]]);
      // "今晚" / "明早" also say which half of the day; put that part back for the time parser.
      if (m[0] === "今晚") text = text.replace(m[0], "晚上");
      else if (m[0] === "明早" || m[0] === "今早") text = text.replace(m[0], "早上");
    });

    if (!date) {
      take(/(下)?(?:周|星期)([一二三四五六日天])/, (m) => {
        const target = WEEK_INDEX[m[2]];
        const current = ((now.getDay() + 6) % 7) + 1;
        let delta = target - current;
        if (m[1]) delta += 7;
        else if (delta < 0) delta += 7;
        date = addDays(today, delta);
      });
    }

    if (!date) {
      take(/(\d{1,2})月(\d{1,2})[日号]/, (m) => {
        const month = Number(m[1]);
        const day = Number(m[2]);
        if (month < 1 || month > 12 || day < 1 || day > 31) return false;
        let year = now.getFullYear();
        if (`${year}-${pad(month)}-${pad(day)}` < today) year += 1;
        date = `${year}-${pad(month)}-${pad(day)}`;
      });
    }

    // "15:30" / "下午3点" / "9点半" / "晚上8点15分"
    take(/(上午|早上|中午|下午|晚上)?\s*(\d{1,2})(?:[:：](\d{2})|[点时](?:(半)|(\d{1,2})分?)?)/, (m) => {
      let h = Number(m[2]);
      const min = m[3] ? Number(m[3]) : m[4] ? 30 : m[5] ? Number(m[5]) : 0;
      if (h > 23 || min > 59) return false;
      const part = m[1];
      if ((part === "下午" || part === "晚上") && h < 12) h += 12;
      if (part === "中午" && h < 11) h += 12;
      time = { h, m: min };
    });

    if (time) {
      const t = time as { h: number; m: number };
      let day = date ?? baseDate;
      // A bare time that has already passed today means tomorrow.
      if (!date && day === today) {
        const candidate = new Date(now.getFullYear(), now.getMonth(), now.getDate(), t.h, t.m);
        if (candidate.getTime() <= now.getTime()) day = addDays(today, 1);
      }
      date = day;
      const [y, mo, d] = day.split("-").map(Number);
      remind = new Date(y, mo - 1, d, t.h, t.m);
    }
  }

  const title = text.replace(/\s+/g, " ").replace(/^[\s,，。:：、]+|[\s,，、]+$/g, "");
  const finalDate = date ?? baseDate;
  const remindAt = remind ? fmtDateTime(remind) : null;

  let hint = "";
  if (date || remindAt) {
    const dayText = relativeDay(finalDate, today);
    const named = /^\d/.test(dayText) ? `${dayText} ${weekday(finalDate)}` : dayText;
    hint = remindAt ? `${named} ${remindAt.slice(11, 16)} 提醒` : named;
  }

  return { title, date: finalDate, remind_at: remindAt, hint };
}
