// Run with: node --test ui/src/lib/quickparse.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseQuick } from "./quickparse.ts";

// Wednesday 2026-11-11 14:20
const now = new Date(2026, 10, 11, 14, 20);
const today = "2026-11-11";

test("plain text stays a title on the viewed day", () => {
  const r = parseQuick("回复客户询盘", today, now);
  assert.deepEqual(r, { title: "回复客户询盘", date: today, remind_at: null, hint: "" });
});

test("viewed day is used when the text names no day", () => {
  const r = parseQuick("整理样品", "2026-11-20", now);
  assert.equal(r.date, "2026-11-20");
  assert.equal(r.remind_at, null);
});

test("tomorrow with a clock time", () => {
  const r = parseQuick("明天 15:00 回访客户", today, now);
  assert.equal(r.title, "回访客户");
  assert.equal(r.date, "2026-11-12");
  assert.equal(r.remind_at, "2026-11-12T15:00:00");
  assert.equal(r.hint, "明天 15:00 提醒");
});

test("afternoon hours are shifted to 24h", () => {
  const r = parseQuick("下午3点半开会", today, now);
  assert.equal(r.title, "开会");
  assert.equal(r.remind_at, "2026-11-11T15:30:00");
});

test("a bare time already past today rolls to tomorrow", () => {
  const r = parseQuick("9点 发报价单", today, now);
  assert.equal(r.date, "2026-11-12");
  assert.equal(r.remind_at, "2026-11-12T09:00:00");
});

test("an explicit 今天 never rolls over", () => {
  const r = parseQuick("今天9点 发报价单", today, now);
  assert.equal(r.date, today);
  assert.equal(r.remind_at, "2026-11-11T09:00:00");
});

test("今晚 means today in the evening", () => {
  const r = parseQuick("今晚8点 录口播", today, now);
  assert.equal(r.title, "录口播");
  assert.equal(r.remind_at, "2026-11-11T20:00:00");
});

test("weekday names pick the next such day", () => {
  assert.equal(parseQuick("周五 交稿", today, now).date, "2026-11-13");
  assert.equal(parseQuick("周一 例会", today, now).date, "2026-11-16");
  assert.equal(parseQuick("下周三 复盘", today, now).date, "2026-11-18");
  assert.equal(parseQuick("周三 复盘", today, now).date, today);
});

test("month and day", () => {
  const r = parseQuick("12月3日 寄样品", today, now);
  assert.equal(r.title, "寄样品");
  assert.equal(r.date, "2026-12-03");
  assert.equal(r.hint, "12月3日 周四");
  assert.equal(parseQuick("1月5号 年审", today, now).date, "2027-01-05");
});

test("relative minutes and hours", () => {
  const r = parseQuick("30分钟后 取快递", today, now);
  assert.equal(r.title, "取快递");
  assert.equal(r.remind_at, "2026-11-11T14:50:00");
  assert.equal(parseQuick("10小时后提醒我喝水", today, now).remind_at, "2026-11-12T00:20:00");
});

test("numbers that are not times are left alone", () => {
  const r = parseQuick("订 3 个样品", today, now);
  assert.equal(r.title, "订 3 个样品");
  assert.equal(r.remind_at, null);
});
