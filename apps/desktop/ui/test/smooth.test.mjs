/**
 * 平滑曲线的**行为级**验证（不是 grep 源码）。
 *
 * 为什么必须这么写：上一版测试只断言源码里出现 `s > 9` 字样，QA 做了三个变异
 * （关掉限幅 / 控制点符号取反 / 废掉断线切段），15 条测试全绿——一条恒真的断言
 * 等于没测。「不过冲」是可计算的数学性质，就该在采样点上算出来。
 *
 * 做法：
 * 1. 从 `src/TrendChart.tsx` 里按花括号配对取出 `smoothPath` / `segmentsOf`
 *    的完整函数体（不是「截到下一个 }」）；
 * 2. 剥掉 TS 注解后用 `new Function` 编译成可执行函数（取不到就 assert.fail，
 *    绝不允许静默跳过）；
 * 3. 把返回的 SVG 路径解析成一段段三次贝塞尔，每段取 t = 0.01…0.99 共 99 个
 *    采样点，断言 y 落在该段两端点 y 的闭区间内 —— 这就是「不过冲」的定义。
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CHART_PATH = path.join(path.resolve(HERE, ".."), "src", "TrendChart.tsx");
const SRC = readFileSync(CHART_PATH, "utf8");

// ---------------------------------------------------------------- 源码提取

/// 从 `openAt` 处的 `{` 开始做花括号配对；跳过注释与字符串，返回配对 `}` 的下标。
function matchBrace(src, openAt) {
  let depth = 0;
  for (let i = openAt; i < src.length; i++) {
    const ch = src[i];
    if (ch === "/" && src[i + 1] === "/") {
      const nl = src.indexOf("\n", i);
      if (nl < 0) return -1;
      i = nl;
      continue;
    }
    if (ch === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      if (end < 0) return -1;
      i = end + 1;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      const quote = ch;
      for (i++; i < src.length; i++) {
        if (src[i] === "\\") {
          i++;
          continue;
        }
        if (src[i] === quote) break;
      }
      continue;
    }
    if (ch === "{") depth++;
    else if (ch === "}") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/// 取 `signature` 这个函数的**函数体**（`{` 与 `}` 之间的原文）。
function bodyOf(src, signature) {
  const start = src.indexOf(signature);
  if (start < 0) return null;
  let i = start + signature.length;
  if (src[i] !== "(") return null;

  // 先跳过参数表（参数里可能有 `{ x: number; y: number }` 这种解构注解）
  let paren = 0;
  for (; i < src.length; i++) {
    if (src[i] === "(") paren++;
    else if (src[i] === ")") {
      paren--;
      if (paren === 0) {
        i++;
        break;
      }
    }
  }
  // 再跨过返回值注解，找到函数体的左花括号
  for (; i < src.length; i++) {
    if (src[i] === "{") break;
    if (src[i] === ";" || src[i] === "=") return null;
  }
  if (src[i] !== "{") return null;

  const end = matchBrace(src, i);
  if (end < 0) return null;
  return src.slice(i + 1, end);
}

/// 去掉函数体里剩下的 TS 注解（本项目只用得到这几种形态）。
function stripTypes(code) {
  return (
    code
      // const d: number[] = [] / let run: number[] = []
      .replace(
        /(\b(?:const|let|var)\s+[A-Za-z_$][\w$]*)\s*:\s*[A-Za-z_$][\w$]*(?:\[\])*(?:\s*\|\s*(?:null|[A-Za-z_$][\w$]*(?:\[\])*))*/g,
        "$1"
      )
      // new Array<number>(n)
      .replace(/new\s+([A-Za-z_$][\w$]*)<[^<>()]*>/g, "new $1")
  );
}

/// 提取 + 编译；任何一步失败都抛出带说明的错误（由调用处转成 assert.fail）。
function compile(src, signature, params) {
  const body = bodyOf(src, signature);
  if (body === null) {
    throw new Error(`无法从源码取出「${signature}」的函数体：函数结构变了，请更新提取器`);
  }
  const js = stripTypes(body);
  try {
    return new Function(...params, js);
  } catch (e) {
    throw new Error(`提取出的函数体不是合法 JS（${e.message}）：\n${js.slice(0, 400)}`);
  }
}

/// 懒加载：提取失败不炸整个文件，而是让用到它的每条测试各自红。
function load(signature, params) {
  try {
    return { fn: compile(SRC, signature, params), error: null };
  } catch (e) {
    return { fn: null, error: e.message };
  }
}

const SMOOTH = load("export function smoothPath", ["pts"]);
const SEGMENTS = load("export function segmentsOf", ["values"]);

function smoothPath(pts) {
  if (SMOOTH.error) assert.fail(SMOOTH.error);
  return SMOOTH.fn(pts);
}

function segmentsOf(values) {
  if (SEGMENTS.error) assert.fail(SEGMENTS.error);
  return SEGMENTS.fn(values);
}

// ---------------------------------------------------------------- 路径解析

/// 把 `M x,y C c1x,c1y c2x,c2y x,y …` 解析成若干段三次贝塞尔。
function parseCubics(d) {
  const tokens = d.match(/[MC]|-?\d+(?:\.\d+)?/g) ?? [];
  const cubics = [];
  let i = 0;
  let cur = null;
  while (i < tokens.length) {
    const t = tokens[i++];
    if (t === "M") {
      cur = { x: Number(tokens[i++]), y: Number(tokens[i++]) };
      continue;
    }
    if (t === "C") {
      const c1 = { x: Number(tokens[i++]), y: Number(tokens[i++]) };
      const c2 = { x: Number(tokens[i++]), y: Number(tokens[i++]) };
      const p3 = { x: Number(tokens[i++]), y: Number(tokens[i++]) };
      assert.ok(cur, "C 段之前必须先有 M");
      cubics.push({ p0: cur, c1, c2, p3 });
      cur = p3;
      continue;
    }
    assert.fail(`路径里出现了非曲线指令 ${t}：${d.slice(0, 120)}`);
  }
  return cubics;
}

/// 三次贝塞尔在 t 处的 y。
function bezierY(seg, t) {
  const mt = 1 - t;
  return (
    mt * mt * mt * seg.p0.y + 3 * mt * mt * t * seg.c1.y + 3 * mt * t * t * seg.c2.y + t * t * t * seg.p3.y
  );
}

/// 跑一组数据，返回最大越界量与最大「偏离直线」量（都在数据单位下）。
function probe(pts) {
  const d = smoothPath(pts);
  const cubics = parseCubics(d);
  assert.equal(cubics.length, pts.length - 1, `应有 ${pts.length - 1} 段曲线，实际 ${cubics.length} 段`);

  let maxOvershoot = 0;
  let maxBend = 0;
  for (const seg of cubics) {
    const lo = Math.min(seg.p0.y, seg.p3.y);
    const hi = Math.max(seg.p0.y, seg.p3.y);
    for (let k = 1; k <= 99; k++) {
      const t = k / 100;
      const y = bezierY(seg, t);
      const over = y < lo ? lo - y : y > hi ? y - hi : 0;
      if (over > maxOvershoot) maxOvershoot = over;

      // 与两端直线插值比：只看中点会漏掉对称的 S 形（它的中点和直线重合）。
      const straight = seg.p0.y + (seg.p3.y - seg.p0.y) * t;
      maxBend = Math.max(maxBend, Math.abs(y - straight));
    }
  }
  return { path: d, cubics, maxOvershoot, maxBend };
}

/// 等距 x 的取值 → 点。
function evenly(values) {
  return values.map((v, i) => ({ x: i * 40, y: v }));
}

// ---------------------------------------------------------------- 测试

/** 四组样本：升降交替 / 大量 0 + 尖峰 / 单调递增 / 满值大幅波动。 */
const CASES = [
  { name: "升降交替 [0,300,250,200,60,10]", values: [0, 300, 250, 200, 60, 10] },
  { name: "0 与尖峰 [0,0,0,3,0,120,8,0,45,3]", values: [0, 0, 0, 3, 0, 120, 8, 0, 45, 3] },
  { name: "单调递增 [5,20,120,400]", values: [5, 20, 120, 400] },
  // 这一组专门守圆盘限幅：α²+β² 明显超 9，关掉限幅会越界 2.14 个单位。
  { name: "大幅波动 [405,371,32,309,248,102]", values: [405, 371, 32, 309, 248, 102] },
];

test("提取器能拿到 smoothPath 且它是可执行函数", () => {
  assert.equal(typeof smoothPath, "function");
  const out = smoothPath([
    { x: 0, y: 0 },
    { x: 40, y: 100 },
    { x: 80, y: 50 },
  ]);
  assert.equal(typeof out, "string");
  assert.ok(out.startsWith("M"), `路径应以 M 开头，实际 ${out.slice(0, 40)}`);
  assert.ok(out.includes("C"), "相邻点之间应是三次贝塞尔（C 段），不是直线");
});

test("平滑不过冲：每段采样点都落在两端 y 的闭区间内", () => {
  const report = [];
  for (const c of CASES) {
    const { maxOvershoot, maxBend } = probe(evenly(c.values));
    report.push(`${c.name} 越界 ${maxOvershoot.toFixed(6)}，弯曲 ${maxBend.toFixed(2)}`);
    assert.ok(
      maxOvershoot < 1e-6,
      `${c.name} 过冲了 ${maxOvershoot.toFixed(4)} 个单位（新增/消耗会被画到 0 以下）：\n${report.join("\n")}`
    );
  }
});

test("随机取值也不过冲（2000 组，含大量 0 与尖峰）", () => {
  // 固定种子的线性同余，保证失败可复现
  let seed = 20261128;
  const rand = () => {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed / 2147483648;
  };
  let worst = 0;
  let worstCase = "";
  for (let round = 0; round < 2000; round++) {
    const n = 3 + Math.floor(rand() * 30);
    const values = Array.from({ length: n }, () => {
      const r = rand();
      // 三分之一的概率落在 0（新增/消耗最常见的值）
      if (r < 0.33) return 0;
      return Math.round(rand() * 500 * (r < 0.66 ? 1 : 10));
    });
    const { maxOvershoot } = probe(evenly(values));
    if (maxOvershoot > worst) {
      worst = maxOvershoot;
      worstCase = JSON.stringify(values);
    }
  }
  assert.ok(worst < 1e-6, `随机样本里最差越界 ${worst.toFixed(6)}：${worstCase}`);
});

test("确实是曲线：至少有一段明显偏离两端直线", () => {
  for (const c of CASES) {
    const { maxBend } = probe(evenly(c.values));
    assert.ok(
      maxBend > 0.5,
      `${c.name} 的每段都贴着直线（最大弯曲 ${maxBend.toFixed(3)}），说明退化成了折线`
    );
  }
});

test("单调递增的样本也保持单调（相邻采样点不回头）", () => {
  const { cubics } = probe(evenly([5, 20, 120, 400]));
  for (const seg of cubics) {
    let prev = seg.p0.y;
    for (let k = 1; k <= 99; k++) {
      const y = bezierY(seg, k / 100);
      assert.ok(y >= prev - 1e-6, `单调序列的曲线回头了：${prev} → ${y}`);
      prev = y;
    }
  }
});

test("断线逻辑：null 把序列切成若干连续段", () => {
  assert.equal(typeof segmentsOf, "function");
  assert.deepEqual(segmentsOf([1, null, 2, 3, null, 4]), [[0], [2, 3], [5]]);
  // 首尾的空值不产生空段；连续空值只切一次
  assert.deepEqual(segmentsOf([null, null, 7, 8]), [[2, 3]]);
  assert.deepEqual(segmentsOf([null, null]), []);
  assert.deepEqual(segmentsOf([]), []);
});

test("line() 真的用 segmentsOf 切段、再逐段 smoothPath", () => {
  // smoothPath/segmentsOf 本身测过了，这条守「调用点没被绕过」：
  // 把 null 直接丢给一段平滑（跨空档连斜线）就是这里失守。
  const at = SRC.indexOf("const line = (");
  assert.ok(at > 0, "找不到 line()：绘图主流程结构变了");
  const chunk = SRC.slice(at, at + 600);
  assert.match(chunk, /segmentsOf\(values\)/, "line() 必须先用 segmentsOf 切段");
  assert.match(chunk, /\.map\(\(idx\) => smoothPath\(/, "每段各自平滑，不能合并成一条路径");
});
