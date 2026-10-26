#!/usr/bin/env python
"""文档一致性校验（RULE §十 的机械部分）。

只查**能机械判定**的四件事：
  1. 交叉引用：当前前缀的 ID（`DsnX`/`DsnE`/`ECS`/`SYN`/`LECS`/… ）必须真的有定义；
  2. 章节引用：`REFACTOR §N` 的 N 必须存在；旧式无前缀编号（`Dsn28` 这类）视为失效；
  3. 条目标记：`<!-- &ID … -->` / `<!-- &/ID -->` / `### ID ` 三者集合一致，且 DESIGN 必须有 `## 主题索引`；
  4. 枚举数量：GAME 的地形表行数 == `TILE_PROPS` 条目数。

历史编号（`I89`/`A41`/`G22`/`P9` 等迁移前的全局编号）只统计不判错——参考对照靠 `git log`。
退出码：1 = 有 FAIL；0 = 通过（只报告项不影响退出码）。
"""

import pathlib
import re
import sys

# Windows 控制台默认 GBK：输出里带 ✓/✗ 会直接 UnicodeEncodeError 让脚本崩掉。
# 强制用 UTF-8 + replace 兜底，并且正文只用 ASCII 记号——不给自己挖坑。
try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
except Exception:  # noqa: BLE001 - 老解释器没有 reconfigure，忽略
    pass

ROOT = pathlib.Path(__file__).resolve().parent.parent

DOC_GLOBS = [
    "*.md", "ecs_core/*.md", "presentation/*.md", "render-api/*.md",
    "tui/*.md", "sys/*.md", "utils/*.md", "archive/*.md", "archive/*/*.md",
]

CURRENT_PREFIXES = ("DsnX", "DsnE", "DsnP", "DsnA", "DsnT", "DsnS", "DsnU",
                    "ECS", "SYN", "PRE", "API", "TUI", "SYS", "UTL",
                    "LECS", "LSYN", "LPRE", "LAPI", "LTUI", "LSYS", "LUTL")
LEGACY_PREFIXES = ("I", "A", "G", "D", "P", "R")

ID_RE = re.compile(r"\b(" + "|".join(CURRENT_PREFIXES) + r")(\d+)\b")
LEGACY_RE = re.compile(r"(?<![A-Za-z])(" + "|".join(LEGACY_PREFIXES) + r")(\d+)\b")
IGNORE = "docs-check:ignore"


def doc_files():
    seen = []
    for pat in DOC_GLOBS:
        for p in sorted(ROOT.glob(pat)):
            if p.is_file() and p not in seen:
                seen.append(p)
    return seen


def main() -> int:
    files = doc_files()
    texts = {p: p.read_text(encoding="utf-8") for p in files}

    # ---- 已定义 ID 集合：`### ID` 标题、表格首列 `| ID |`、`<!-- &ID` 标记 ----
    defined = set()
    for p, t in texts.items():
        # 标题层级：DESIGN 用 `### ID`、GAME 用 `## GmN`，两级都认
        for m in re.finditer(r"^#{2,3} ([A-Za-z]+\d+)\b", t, re.M):
            defined.add(m.group(1))
        for m in re.finditer(r"^\| ([A-Za-z]+\d+) \|", t, re.M):
            defined.add(m.group(1))
        for m in re.finditer(r"^<!-- &([A-Za-z]+\d+) ", t, re.M):
            defined.add(m.group(1))

    fails, reports = [], []
    legacy_hits = 0

    for p, t in texts.items():
        rel = p.relative_to(ROOT).as_posix()
        # `archive/` 是历史记录，本来就用来点名"已被删掉的东西"，不做交叉引用校验
        skip_xref = rel.startswith("archive/")
        for lineno, line in enumerate(t.splitlines(), 1):
            if IGNORE in line:
                continue
            # 编号公约行（`**编号：** DsnA1、DsnA2…`）是**约定示例**，不是引用
            if line.lstrip().startswith("**编号：**"):
                continue
            legacy_hits += len(LEGACY_RE.findall(line))
            if skip_xref:
                continue
            for m in ID_RE.finditer(line):
                eid = m.group(0)
                # 「原 ECS33（已并入…）」这类**墓碑引用**是合法的：显式指向已删除条目
                if re.search(r"原\s*" + re.escape(eid), line):
                    continue
                # `Dsn28` 这类无类别字母的旧写法不在 ID_RE 里，另行检查
                if eid not in defined:
                    fails.append(f"{rel}:{lineno}  未定义的引用 {eid}")

        # 旧式无前缀编号（Dsn12、Dsn28…）
        for m in re.finditer(r"\bDsn(\d+)\b", t):
            lineno = t[: m.start()].count("\n") + 1
            line = t.splitlines()[lineno - 1]
            if IGNORE in line:
                continue
            fails.append(f"{rel}:{lineno}  失效编号 {m.group(0)}（应为 DsnX/DsnE + 数字）")

    # ---- REFACTOR 章节引用 ----
    refactor = texts.get(ROOT / "REFACTOR.md", "")
    sections = set(re.findall(r"^#{2,4} (?:(\d+(?:\.\d+)*)\.?|§?(\d+(?:\.\d+)*))", refactor, re.M))
    valid_sections = {a or b for a, b in sections}
    for p, t in texts.items():
        rel = p.relative_to(ROOT).as_posix()
        for m in re.finditer(r"REFACTOR(?:\.md)?\s*§\s*(\d+(?:\.\d+)*)", t):
            if m.group(1) not in valid_sections:
                lineno = t[: m.start()].count("\n") + 1
                fails.append(f"{rel}:{lineno}  REFACTOR 无 §{m.group(1)} 行")

    # ---- 条目标记配对（仅对已启用标记的文件） ----
    for p, t in texts.items():
        rel = p.relative_to(ROOT).as_posix()
        opens = re.findall(r"^<!-- &([A-Za-z]+\d+) ", t, re.M)
        closes = re.findall(r"^<!-- &/([A-Za-z]+\d+) -->", t, re.M)
        heads = re.findall(r"^#{2,3} ([A-Za-z]+\d+) ", t, re.M)
        if not opens and not closes:
            continue
        if sorted(opens) != sorted(closes):
            missing = set(opens) ^ set(closes)
            fails.append(f"{rel}  标记开合不配对：{sorted(missing)}")
        if sorted(opens) != sorted(heads):
            fails.append(f"{rel}  标记与标题 ID 不一致：{sorted(set(opens) ^ set(heads))}")
        if p.name == "DESIGN.md" and "## 主题索引" not in t:
            fails.append(f"{rel}  缺少 `## 主题索引`")

    # ---- 枚举数量：GAME 地形表 vs TILE_PROPS ----
    game = texts.get(ROOT / "GAME.md", "")
    tile_rs = (ROOT / "ecs_core/src/map/tile.rs")
    if game and tile_rs.exists():
        # 只数表行（缩进 4 空格的 `TileProps {`），排除结构体定义与返回类型
        props = len(re.findall(r"^\s{4}TileProps \{$", tile_rs.read_text(encoding="utf-8"), re.M))
        seg = game.split("### 环境 Tile", 1)
        if len(seg) == 2:
            table = seg[1].split("###", 1)[0]
            rows = len(re.findall(r"^\|\s*\d+\s*\|", table, re.M))
            if rows != props:
                fails.append(f"GAME.md 地形表 {rows} 行 ≠ TILE_PROPS {props} 条")
            else:
                reports.append(f"地形表 = TILE_PROPS = {props} 条 ✓")
        else:
            reports.append("GAME.md 未找到「### 环境 Tile」段，跳过枚举校验")

    # ---- .ps1 必须是 UTF-8 with BOM ----
    # 这不是洁癖：PowerShell 5.1 在无 BOM 时按系统 ANSI 代码页（中文 Windows = GBK）解析，
    # 中文注释会直接让脚本语法错误——编辑工具保存一次就可能把 BOM 吃掉。
    for p in sorted((ROOT / "scripts").glob("*.ps1")):
        if not p.read_bytes().startswith(b"\xef\xbb\xbf"):
            fails.append(
                f"{p.relative_to(ROOT).as_posix()}  缺少 UTF-8 BOM"
                "（PowerShell 5.1 会按 GBK 解析中文注释而报语法错）"
            )

    # ---- 状态/代价欠账（只报告） ----
    for name in ("DESIGN.md", "ecs_core/DESIGN.md"):
        p = ROOT / name
        if not p.exists():
            continue
        t = texts.get(p, "")
        entries = len(re.findall(r"^### (?:Dsn[A-Z])\d+ ", t, re.M))
        status = len(re.findall(r"^\*\*状态：\*\*", t, re.M))
        cost = len(re.findall(r"^\*\*代价", t, re.M))
        reports.append(f"{name}: 条目 {entries} / 状态段 {status} / 代价段 {cost}（欠账不阻塞）")

    reports.append(f"迁移前历史编号引用 {legacy_hits} 处（不校验，靠 git log 追溯）")

    print("=== 文档一致性校验（RULE §十）===")
    for r in reports:
        print("  ·", r)
    if fails:
        print(f"\nFAIL {len(fails)} 项：")
        for f in fails[:40]:
            print("  ✗", f)
        if len(fails) > 40:
            print(f"  … 另有 {len(fails) - 40} 项")
        return 1
    print("\nPASS：交叉引用 / 章节引用 / 标记配对 / 枚举数量 全部一致")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
