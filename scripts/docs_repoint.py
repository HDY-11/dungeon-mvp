"""文档治理：把指向旧 REFACTOR 章节的引用批量改指到新归属。

只做**机械替换**，不做语义改写。默认 dry-run（只报告会改多少处）；
加 `--write` 才落盘。全部读写显式 UTF-8，避免编码往返损坏。
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC_GLOBS = ["*.md", "ecs_core/*.md", "archive/*.md", "archive/*/*.md",
             "presentation/*.md", "render-api/*.md", "tui/*.md", "sys/*.md", "utils/*.md"]

# (旧引用片段, 新引用) —— 顺序有意义：先长后短，避免短模式吃掉长模式的一部分。
RULES = [
    # 契约类：折进 DESIGN DsnX17
    ("REFACTOR.md §10.5 的冻结清单", "DESIGN DsnX17 的冻结清单"),
    ("REFACTOR.md §10.5", "DESIGN DsnX17"),
    ("REFACTOR §10.5", "DESIGN DsnX17"),
    ("REFACTOR §10.7", "DESIGN DsnX17"),
    ("REFACTOR.md §10.7", "DESIGN DsnX17"),
    ("§10.5「不再维护」", "DsnX17「旧实现只在 archive/」"),
    ("照 §10.7「行动实体不存档」的既有惯例", "照 DESIGN DsnX17 ⑥「瞬态子实体不存档」"),
    ("见 REFACTOR §10.7）", "见 DESIGN DsnX17 ③）"),
    # 阶段记录类：折进 REFACTOR §1 已完成表
    ("REFACTOR.md §11.3 Phase H（H1）", "REFACTOR §1（Phase H1）"),
    ("REFACTOR.md §11.3 Phase H（H3）", "REFACTOR §1（Phase H3）"),
    ("REFACTOR.md §11.3 Phase H（H2）", "REFACTOR §1（Phase H2）"),
    ("REFACTOR.md §11.3 Phase H（H4）", "REFACTOR §1（Phase H4）"),
    ("REFACTOR.md §11.3 Phase H（H8 / H9 / H11）", "REFACTOR §2.1（H9/H11 未完成）"),
    ("REFACTOR.md §11.3 Phase H（H10/H12）", "REFACTOR §2.1（H10/H12 未完成）"),
    ("REFACTOR.md §11.3 Phase H（H4/H9/H14）", "REFACTOR §2.1（H9/H14 未完成）"),
    ("REFACTOR.md §11.3 Phase H", "REFACTOR §2.1（Phase H 剩余）"),
    ("REFACTOR §11.3 Phase H（H4）", "REFACTOR §1（Phase H4）"),
    ("REFACTOR §11.3 Phase H（H1）", "REFACTOR §1（Phase H1）"),
    ("REFACTOR §11.3 Phase H（H2）", "REFACTOR §1（Phase H2）"),
    ("REFACTOR §11.3 Phase H（H9/H11）", "REFACTOR §2.1（H9/H11 未完成）"),
    ("REFACTOR §11.3 Phase H（H1/H3", "REFACTOR §1（Phase H1/H3"),
    ("REFACTOR §11.3 Phase H", "REFACTOR §2.1（Phase H 剩余）"),
    ("REFACTOR.md §11.3 Phase C（C5）", "REFACTOR §1（Phase C）"),
    ("REFACTOR §11.3 Phase C8", "REFACTOR §1（Phase C）"),
    ("REFACTOR §11.3 Phase C（挂载与推进两段式）", "DESIGN DsnE8（挂载与推进两段式）"),
    ("REFACTOR §11.3 Phase C", "REFACTOR §1（Phase C）"),
    ("REFACTOR.md §11.3 Phase C", "REFACTOR §1（Phase C）"),
    ("REFACTOR §11.3 Phase D", "REFACTOR §1（Phase D）"),
    ("REFACTOR.md §11.3 Phase D（D3 迁移记录）", "REFACTOR §1（Phase D）"),
    ("REFACTOR.md §11.3 Phase A", "REFACTOR §1（Phase A）"),
    ("REFACTOR.md §11.3 Phase F（F2）", "REFACTOR §1（Phase F）"),
    ("REFACTOR.md §11.3 Phase F5", "REFACTOR §1（Phase F）"),
    ("REFACTOR.md §11.3 Phase F", "REFACTOR §1（Phase F）"),
    ("REFACTOR §11.3（Phase H 的开工顺序）", "REFACTOR §2"),
    ("REFACTOR §11.3", "REFACTOR §1"),
    ("REFACTOR.md §11.3", "REFACTOR §1"),
    ("§11.6 第 6 项", "REFACTOR §1（Phase C 的决策确认）"),
    ("§11.6 第 5 项「先按旧敏捷保行为」", "REFACTOR §1（Phase D 的决策确认）"),
    ("§11.6 第 4 项", "REFACTOR §1（Phase D 的决策确认）"),
    ("§11.6 第 3 项", "REFACTOR §1（Phase D 的决策确认）"),
    ("REFACTOR §11.6 第 3 项", "REFACTOR §1（Phase D 的决策确认）"),
    ("§11.6", "REFACTOR §1"),
    ("REFACTOR.md §11.4 测试矩阵", "REFACTOR §1"),
    ("REFACTOR.md §11.4", "REFACTOR §1"),
    ("REFACTOR.md §10.4 / §10.6 第 4、5 项", "REFACTOR §3"),
    ("REFACTOR.md §10.4 / §10.6 第 2 项 / §11.3 Phase A", "REFACTOR §1（Phase A）"),
    ("REFACTOR.md §10.4 / §10.6 第 4 项 / §11.3 Phase F（F2）", "REFACTOR §1（Phase F）"),
    ("REFACTOR.md §10.4 / §11.4 测试矩阵", "REFACTOR §1"),
    ("REFACTOR.md §10.6 第 1 项", "REFACTOR §1（Phase F）"),
    ("§10.6 第 1 项", "REFACTOR §1（Phase F）"),
    ("§10.6", "REFACTOR §2"),
    ("REFACTOR.md §10.8", "ISSUES ECS25"),
    ("§10.8", "ISSUES ECS25"),
    # 行动系统设计本体 → DsnE8
    ("REFACTOR.md §2.6 / §3.6 / §8.1 / §10.6 / §10.8", "DESIGN DsnE8"),
    ("REFACTOR.md §3.6.8", "DESIGN DsnE8"),
    ("REFACTOR.md §3.6", "DESIGN DsnE8"),
    ("REFACTOR §3.6.7", "DESIGN DsnE8"),
    ("REFACTOR §3.6", "DESIGN DsnE8"),
    ("REFACTOR.md §3.6 / §8.1", "DESIGN DsnE8"),
    ("REFACTOR.md §2.6 / §3.6 / §10.6", "DESIGN DsnE8"),
    ("REFACTOR.md §2.6 / §3.6.7 / §11.3 Phase D / §11.6", "DESIGN DsnE8；REFACTOR §1（Phase D）"),
    ("REFACTOR.md §2.6 / §3.6", "DESIGN DsnE8"),
    ("REFACTOR §2.6", "DESIGN DsnE8 ②"),
    ("REFACTOR.md §2.6", "DESIGN DsnE8 ②"),
    ("REFACTOR.md §5", "DESIGN DsnE8"),
    # 渲染后端摘要 → DsnX14
    ("REFACTOR.md §12 R5", "DESIGN DsnX14（R5）"),
    ("REFACTOR §12", "DESIGN DsnX14"),
    # 文档体系 → RULE §七
    ("REFACTOR.md §13", "RULE.md §七"),
    ("REFACTOR.md §9", "DESIGN DsnE8"),
]


def target_files():
    seen = []
    for pattern in DOC_GLOBS:
        for path in sorted(ROOT.glob(pattern)):
            if path.is_file() and path not in seen:
                seen.append(path)
    return seen


def main() -> int:
    write = "--write" in sys.argv
    total = 0
    for path in target_files():
        text = path.read_text(encoding="utf-8")
        original = text
        hits = []
        for old, new in RULES:
            count = text.count(old)
            if count:
                hits.append((old, new, count))
                text = text.replace(old, new)
        if text != original:
            changed = sum(h[2] for h in hits)
            total += changed
            print(f"{path.relative_to(ROOT)}: {changed} 处")
            for old, new, count in hits:
                print(f"    {count}x  {old!r} -> {new!r}")
            if write:
                path.write_text(text, encoding="utf-8", newline="")
    print(f"\n合计 {total} 处替换 ({'已落盘' if write else 'dry-run，加 --write 落盘'})")

    # 收尾自检：列出仍指向 REFACTOR 具体章节号的引用（应当为空）。
    import re

    leftovers = []
    for path in target_files():
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(r"REFACTOR[\w. ]{0,4}\s*§\s*[\d.]+", text):
            line = text[: match.start()].count("\n") + 1
            leftovers.append(f"{path.relative_to(ROOT)}:{line}  {match.group(0)!r}")
    if leftovers:
        print("\n仍指向 REFACTOR 章节号的引用：")
        for item in leftovers:
            print("   ", item)
    else:
        print("\n自检通过：没有留下指向 REFACTOR 具体章节号的引用。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
