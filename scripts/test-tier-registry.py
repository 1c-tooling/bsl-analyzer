#!/usr/bin/env python3
"""Check the licensing registry, Cargo metadata and provenance disclosures.

Run from any directory with Python 3.11+: python3 scripts/test-tier-registry.py.
Historical checks require the repository's full Git history; no CI policy is added.
"""

from collections import Counter
import json
from pathlib import Path
import re
import subprocess
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[1]
BASE = "51ac6d89545902345142cc748b8c820ff3f3f314"
AUDIT_BASE = "046ee5d38b74"
PURPOSES = {
    "bsl-config": ("configurations", "ConfigId"),
    "bsl-types": ("Type kernel",),
    "code-chunk": ("Splitting sources", "fragments"),
    "ide-host-core": ("Shared analysis host",),
    "parser-error": ("Parse error types",),
    "vcs": ("Git diff", "analysis scoping"),
}
ADDITIONS = set(PURPOSES)
AUDIT_PATH = "docs/legal/tier-a-provenance-audit.md"
STANDARD = "crates/hir-def/src/module_structure/standard.rs"
OLD_STANDARD = "crates/ide-diagnostics/src/utils/standard_regions.rs"
ATTRIBUTION = {
    "syntax": ("architecture",),
    "base-db": ("concurrent map", "Salsa"),
    "stdx": ("quality of service", "worker pool"),
    "paths": ("absolute", "relative", "wrapper"),
    "cfg": ("HIR", "separate notice"),
    "dataflow": ("MIR dataflow", "type-inference lattice"),
    "hir-def": ("documentation", "DefWithBodyId", "path resolution"),
    "hir-ty": ("type inference", "diagnostics"),
    "hir": ("representation of definitions",),
    "ide-db": ("LineIndex", "Salsa"),
}


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True)


def historical(path, revision=BASE):
    return command("git", "show", f"{revision}:{path}")


def section(text, start, end=None):
    result = text.split(start, 1)[1]
    return result.split(end, 1)[0] if end else result


def rows(text, tier):
    end = "## Tier B" if tier == "A" else "## Clean-room"
    block = section(text, f"## Tier {tier}", end)
    return [line.split("|")[1:-1] for line in block.splitlines() if line.startswith("| `")]


def names(text, tier):
    return [name for row in rows(text, tier) for name in re.findall(r"`([^`]+)`", row[0])]


def dependency_marks(text):
    result = {}
    for row in rows(text, "A"):
        members = re.findall(r"`([^`]+)`", row[0])
        cell = row[2].strip()
        if cell.startswith("all three:"):
            result.update({name: set(re.findall(r"`([^`]+)`", cell)) for name in members})
        elif ":" in cell:
            for clause in cell.split(";"):
                owners, targets = clause.split(":", 1)
                targets = set(re.findall(r"`([^`]+)`", targets))
                if "also" in clause:
                    targets |= next(iter(result[name] for name in members if name in result))
                for name in re.findall(r"`([^`]+)`", owners):
                    result[name] = targets
        else:
            result.update({name: set(re.findall(r"`([^`]+)`", cell)) for name in members})
        assert set(members) <= result.keys(), f"unparsed dependency marks: {row}"
    return result


def closure(graph, name):
    reached = set()
    pending = list(graph[name])
    while pending:
        current = pending.pop()
        if current not in reached:
            reached.add(current)
            pending.extend(graph[current])
    return reached


def qualified_permission(text):
    intro = " ".join(section(text, "## Tier A", "| Crate |").split())
    return all(term in intro for term in (
        "crate's own sources, in isolation", "permission does not extend to a build",
        "depends on the tiers", "dependency tree", "cfg", "bsl-metadata", "under review",
    ))


def notice_entries(text):
    heritage = section(text, "Architecture heritage", "Language heritage")
    entries = re.findall(r"^  \* crates/([\w-]+) — (.*?)(?=^  \*|\n\n)", heritage, re.M | re.S)
    return {name: " ".join(body.split()) for name, body in entries}


def disclosure_complete(text):
    try:
        one = section(text, "## 1.", "## 2.")
        three = section(text, "## 3.", "## 4.")
        four = section(text, "## 4.", "## 5.")
        five = section(text, "## 5.", "## 6.")
        six = section(text, "## 6.", "## 7.")
        transfer = section(text, "### 8.3.", "### 8.4.")
    except IndexError:
        return False
    requirements = (
        (one, (AUDIT_BASE, "31", "нынешняя таблица")),
        (three, ("разобран в #154", "основание Tier A", "repository.workspace = true")),
        (four, ("Не закрывает `cfg` и `bsl-metadata`", "§ 8.3")),
        (five, ("15 файлов", "Architecture heritage", "в исходники", "не возвращались")),
        (six, ("Шесть крейтов", "не покрыты", "не сборку")),
        (transfer, (
            STANDARD.removeprefix("crates/"), OLD_STANDARD.removeprefix("crates/"),
            "05dc4f98", "65f1278a", "843b00ab", "effab845", "R084", "Regions.java",
            "Keywords.java", "#455", "https://its.1c.ru/db/v8std#content:455",
            "https://v8std.ru/std/455/", "§ 1.4", "§ 1.5", "§ 1.6", "§ 1.7",
            "Расхождения со стандартом", "регистронезависимость", "пустой суффикс",
            "Независимого clean-room-переписывания словаря не было",
            "словарь состоит из имён", "обвязка сопоставления и тесты при файле местные",
            "Основанием не служат", "--diff-filter=R -M", "cb6e7ac1", "**D** + **A**",
            "bsl-clean-room-slice-b2.md", "копирования, разделения и пересказы без R",
            "Тир `cfg` и `bsl-metadata` этим не",
        )),
    )
    return all(term in block for block, terms in requirements for term in terms)


class TierRegistryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.licensing = (ROOT / "LICENSING.md").read_text()
        cls.audit = (ROOT / AUDIT_PATH).read_text()
        cls.notice = (ROOT / "NOTICE").read_text()
        cls.baseline = historical("LICENSING.md")
        metadata = json.loads(command("cargo", "metadata", "--no-deps", "--offline", "--format-version", "1"))
        cls.packages = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
        cls.graph = {
            name: {d["name"] for d in p["dependencies"] if d["kind"] in (None, "build") and d["name"] in cls.packages}
            for name, p in cls.packages.items()
        }

    def test_I1_permission_is_for_sources_not_the_build(self):
        self.assertTrue(qualified_permission(self.licensing))
        self.assertFalse(qualified_permission(self.baseline))
        self.assertIn("parser", self.graph["base-db"])
        cfg_users = {n for n, p in self.packages.items() if any(d["name"] == "cfg" and d["kind"] is None for d in p["dependencies"])}
        self.assertEqual(cfg_users, {"dataflow", "hir", "hir-ty"})
        metadata_users = sorted(n for n, p in self.packages.items() if any(d["name"] == "bsl-metadata" and d["kind"] is None for d in p["dependencies"]))
        self.assertEqual(len(metadata_users), 14)
        self.assertIn("fourteen crates depend on", self.licensing)
        print("E1 bsl-metadata direct normal users:", metadata_users)

    def test_I1_dependency_column_matches_normal_build_closure(self):
        marks = dependency_marks(self.licensing)
        tier_b = set(names(self.licensing, "B"))
        for name in names(self.licensing, "A"):
            with self.subTest(crate=name):
                self.assertEqual(marks[name], closure(self.graph, name) & tier_b)
        self.assertEqual(marks["syntax"], {"lexer"})
        self.assertNotIn("parser", marks["syntax"], "syntax's dev dependency must not enter the closure")
        print("E1 Tier B dependency column checked for", len(marks), "crates")

    def test_I2_six_rows_license_inheritance_and_existing_tiers(self):
        a, b = names(self.licensing, "A"), names(self.licensing, "B")
        self.assertEqual(Counter(a), Counter(names(self.baseline, "A")) + Counter(ADDITIONS))
        self.assertEqual(Counter(b), Counter(names(self.baseline, "B")))
        purposes = {row[0].strip().strip("`"): row[1] for row in rows(self.licensing, "A")}
        for name in ADDITIONS:
            with self.subTest(crate=name):
                for term in PURPOSES[name]:
                    self.assertIn(term, purposes[name])
                self.assertEqual(a.count(name), 1)
                self.assertNotIn(name, b)
                manifest = tomllib.loads(Path(self.packages[name]["manifest_path"]).read_text())
                self.assertIs(manifest["package"]["license"]["workspace"], True)
                self.assertEqual(self.packages[name]["license"], "MIT OR Apache-2.0")
                self.assertNotIn(name, names(self.baseline, "A"))
        print("E1 packages/A/B/missing:", len(self.packages), len(a), len(b), sorted(set(self.packages) - set(a + b)))

    def test_I3_I5_audit_sections_and_historical_boundary(self):
        self.assertTrue(disclosure_complete(self.audit))
        self.assertFalse(disclosure_complete(historical(AUDIT_PATH)))
        for term in (
            AUDIT_BASE, "разобран в #154", "Не закрывает `cfg` и `bsl-metadata`",
            "Architecture heritage", "не покрыты", "словарь состоит из имён",
            "--diff-filter=R -M", "копирования, разделения и пересказы без R",
        ):
            with self.subTest(omitted_disclosure=term):
                self.assertFalse(disclosure_complete(self.audit.replace(term, "")))
        ledger = section(self.audit, "Ведомость удалённых строк", "## 6.")
        counts = {}
        files = set()
        for line in ledger.splitlines():
            if line.startswith("| `"):
                cells = line.split("|")
                crate = cells[1].strip().strip("`")
                self.assertNotIn(crate, counts)
                counts[crate] = int(cells[3])
                files.update(f"crates/{crate}/{p}" for p in re.findall(r"`([^`]+)`", cells[2]))
        boundary = names(historical("LICENSING.md", AUDIT_BASE), "A")
        patch = command("git", "show", "843b00ab", "--", *[f"crates/{n}" for n in boundary])
        hits = []
        path = ""
        for line in patch.splitlines():
            if line.startswith("diff --git"):
                path = line.split()[2][2:]
            if line.startswith("-") and not line.startswith("---") and "rust-analyzer" in line:
                hits.append((path, line))
        self.assertEqual({p for p, _ in hits}, files)
        self.assertEqual(Counter(p.split("/")[1] for p, _ in hits), counts)
        self.assertEqual(set(counts), set(ATTRIBUTION))
        self.assertEqual((len(hits), len(counts), len({p for p, _ in hits})), (17, 10, 15))
        self.assertIn(("crates/syntax/src/lib.rs", "-//! Based on rust-analyzer's syntax crate architecture."), hits)
        print("E3 boundary:", len(boundary), "crates; calibration: 17 lines / 10 crates / 15 files")
        for path, line in hits:
            print("E3", path, line)

    def test_I3_rename_search_both_directions_and_nonrename_limit(self):
        a, b = set(names(self.licensing, "A")), set(names(self.licensing, "B"))
        log = command("git", "log", "HEAD", "--format=COMMIT %H", "--name-status", "--diff-filter=R", "-M", "--", "crates", "xtask")
        found = []
        commit = ""
        for line in log.splitlines():
            if line.startswith("COMMIT "):
                commit = line.split()[1]
            if line.startswith("R"):
                score, old, new = line.split("\t")
                x, y = (p.split("/")[1] if p.startswith("crates/") else "xtask" for p in (old, new))
                if (x in a and y in b) or (x in b and y in a):
                    found.append((commit, score, old, new, "A→B" if x in a else "B→A"))
        transfer_commit = command("git", "rev-parse", "effab845^{commit}").strip()
        self.assertIn((transfer_commit, "R084", OLD_STANDARD, STANDARD, "B→A"), found)
        count_a = sum(r[-1] == "A→B" for r in found)
        count_b = sum(r[-1] == "B→A" for r in found)
        self.assertIn(f"**A→B {count_a}, B→A {count_b}**", self.audit)
        limit = command("git", "show", "--format=", "--name-status", "-M", "cb6e7ac1", "--", "crates")
        self.assertIn("D\tcrates/ide-diagnostics/src/utils/preprocessor_symbols.rs", limit)
        self.assertIn("A\tcrates/syntax/src/preproc_symbols.rs", limit)
        print("E2", *found, sep="\n")
        print("E2 A→B / B→A:", count_a, count_b, "; D/A positive limit confirmed")

    def test_I4_hir_ty_repository_inherits_our_workspace(self):
        manifest = tomllib.loads((ROOT / "crates/hir-ty/Cargo.toml").read_text())
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]
        self.assertEqual(manifest["package"]["repository"], {"workspace": True})
        self.assertEqual(self.packages["hir-ty"]["repository"], workspace["repository"])
        self.assertEqual(workspace["repository"], "https://github.com/itrous/bsl-analyzer")
        self.assertNotEqual(tomllib.loads(historical("crates/hir-ty/Cargo.toml"))["package"]["repository"], workspace["repository"])
        print("E1 hir-ty repository:", self.packages["hir-ty"]["repository"])

    def test_I5_notice_has_each_specific_attribution(self):
        entries = notice_entries(self.notice)
        self.assertEqual(set(entries), set(ATTRIBUTION))
        architecture = section(self.notice, "Architecture heritage", "Language heritage")
        self.assertEqual(len(re.findall(r"^  \* crates/", architecture, re.M)), len(ATTRIBUTION))
        self.assertEqual(notice_entries(historical("NOTICE")), {})
        for name, terms in ATTRIBUTION.items():
            with self.subTest(crate=name):
                for term in terms:
                    self.assertIn(term, entries[name])
        architecture = section(self.notice, "Architecture heritage", "Language heritage")
        self.assertIn("https://github.com/rust-analyzer/rust-analyzer", architecture)
        self.assertIn("MIT OR Apache-2.0", architecture)
        self.assertIn("no more of the crate", architecture)
        self.assertIn(AUDIT_PATH, architecture)
        print("I5 NOTICE:", sorted(entries))

    def test_I6_only_repository_changed_in_product_and_ci_files(self):
        paths = ["crates", "xtask", "Cargo.toml", "Cargo.lock", ".github", ".gitlab-ci.yml"]
        changed = command("git", "diff", "--name-only", BASE, "--", *paths).splitlines()
        untracked = command("git", "ls-files", "--others", "--exclude-standard", "--", *paths).splitlines()
        self.assertEqual(untracked, [], "untracked product/CI files must not escape the comparison")
        self.assertEqual(changed, ["crates/hir-ty/Cargo.toml"])
        before = historical("crates/hir-ty/Cargo.toml")
        after = (ROOT / "crates/hir-ty/Cargo.toml").read_text()
        self.assertEqual(after, before.replace('repository = "https://github.com/1c-syntax/bsl-analyzer"', "repository.workspace = true"))
        self.assertEqual((ROOT / STANDARD).read_text(), historical(STANDARD))
        print("I6 product/CI diff:", changed, "; only repository; standard.rs byte-identical")


if __name__ == "__main__":
    unittest.main(verbosity=2)
