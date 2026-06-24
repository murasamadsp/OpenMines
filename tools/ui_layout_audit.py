#!/usr/bin/env python3
"""Audit Unity UI layout risk points.

This is intentionally read-only. It scans Unity scene/prefab YAML and C# UI
scripts for patterns that often break across resolutions: fixed RectTransform
sizes, point anchors, hardcoded positions, CanvasScaler settings, and runtime
sizeDelta/localPosition mutations.
"""

from __future__ import annotations

import argparse
import re
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import NamedTuple


ROOT = Path(__file__).resolve().parents[1]
CLIENT = ROOT / "client"

YAML_GLOBS = [
    "Assets/Scenes/**/*.unity",
    "Assets/Prefabs/**/*.prefab",
]

SCRIPT_GLOBS = [
    "Assets/Scripts/UI/**/*.cs",
    "Assets/Scripts/Gameplay/**/*.cs",
]

SERVER_HORB_GLOBS = [
    "server/net/session/**/*.rs",
]

PATTERNS = {
    "canvas_scaler": re.compile(r"m_UiScaleMode|m_ReferenceResolution|m_MatchWidthOrHeight"),
    "anchor": re.compile(r"m_Anchor(?:Min|Max): \{x: ([^,]+), y: ([^}]+)\}"),
    "size_delta": re.compile(r"m_SizeDelta: \{x: ([^,]+), y: ([^}]+)\}"),
    "anchored_position": re.compile(r"m_AnchoredPosition: \{x: ([^,]+), y: ([^}]+)\}"),
    "runtime_size": re.compile(r"\.sizeDelta\s*=|new Vector2\((?:[^)]*)\)"),
    "runtime_position": re.compile(r"\.(?:localPosition|anchoredPosition)\s*="),
    "layout_rebuild": re.compile(r"ForceRebuildLayoutImmediate|ContentSizeFitter|LayoutGroup|ScrollRect"),
    "screen_dependency": re.compile(r"Screen\.(?:width|height|dpi|safeArea)"),
}

DESIGN_WIDTH = 1920.0
DESIGN_HEIGHT = 1080.0
UI_SIZE = 0.75
UI_LARGE_BOOST = 1.25
WORLD_ZOOM_BASE = 1.3
TERRAIN_TILE_SIZE = 16.0
MIN_WORLD_TILE_PIXELS = 8.0
REFERENCE_SCREEN_INCHES = 12.5
DENSITY_BOOST_EXPONENT = 0.85
DENSITY_BOOST_MAX = 2.2
DESKTOP_DENSITY_BOOST_MAX = 2.05
MAC_RETINA_FALLBACK_SHORT_SIDE_INCHES = 7.4
MAC_RETINA_FALLBACK_MAX_SHORT_SIDE = 2240

DEFAULT_SCALE_RESOLUTIONS = [
    (1280, 720),
    (1366, 768),
    (1512, 982),
    (3024, 1964),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3440, 1440),
    (1280, 1024),
    (1024, 768),
]

DEFAULT_SCALE_DPI = [0, 96, 110, 160, 220]


class ScaleRow(NamedTuple):
    width: int
    height: int
    dpi: int
    density: float
    reference_width: float
    reference_height: float
    resolution_scale: float
    world_tile_pixels: float


@dataclass
class GameObjectInfo:
    file_id: str
    name: str
    components: list[str]
    path: Path


@dataclass
class RectTransformInfo:
    file_id: str
    game_object_id: str
    anchor_min: str
    anchor_max: str
    anchored_position: str
    size_delta: str
    pivot: str
    father: str
    path: Path


class FitRow(NamedTuple):
    name: str
    width: int
    height: int
    dpi: int
    canvas_scale: float
    pixel_width: float
    pixel_height: float
    margin_x: float
    margin_y: float


@dataclass
class ScriptUsageInfo:
    guid: str
    path: Path
    reference_count: int


def iter_files(globs: list[str]) -> list[Path]:
    files: list[Path] = []
    for glob in globs:
        files.extend(CLIENT.glob(glob))
    return sorted(p for p in files if p.is_file())


def iter_root_files(globs: list[str]) -> list[Path]:
    files: list[Path] = []
    for glob in globs:
        files.extend(ROOT.glob(glob))
    return sorted(p for p in files if p.is_file())


def parse_mapping_line(block: str, key: str) -> str:
    match = re.search(rf"^\s*{re.escape(key)}:\s*(.+)$", block, re.MULTILINE)
    return match.group(1).strip() if match else ""


def parse_ref(mapping: str) -> str:
    match = re.search(r"\{fileID:\s*([^,}]+)", mapping)
    return match.group(1).strip() if match else ""


def parse_yaml_objects(
    files: list[Path],
) -> tuple[dict[str, GameObjectInfo], dict[str, RectTransformInfo]]:
    objects: dict[str, GameObjectInfo] = {}
    rects: dict[str, RectTransformInfo] = {}
    for path in files:
        text = path.read_text(errors="replace")
        for raw_block in re.split(r"^--- !u!", text, flags=re.MULTILINE):
            if not raw_block.strip():
                continue
            header, _, body = raw_block.partition("\n")
            header_parts = header.strip().split("&", 1)
            if len(header_parts) != 2:
                continue
            unity_type = header_parts[0].strip()
            file_id = header_parts[1].strip()
            block = body
            if unity_type == "1":
                name = parse_mapping_line(block, "m_Name")
                components = re.findall(r"component:\s*\{fileID:\s*([^,}]+)", block)
                objects[file_id] = GameObjectInfo(file_id, name, components, path)
            elif unity_type == "224":
                rects[file_id] = RectTransformInfo(
                    file_id=file_id,
                    game_object_id=parse_ref(parse_mapping_line(block, "m_GameObject")),
                    anchor_min=parse_mapping_line(block, "m_AnchorMin"),
                    anchor_max=parse_mapping_line(block, "m_AnchorMax"),
                    anchored_position=parse_mapping_line(block, "m_AnchoredPosition"),
                    size_delta=parse_mapping_line(block, "m_SizeDelta"),
                    pivot=parse_mapping_line(block, "m_Pivot"),
                    father=parse_ref(parse_mapping_line(block, "m_Father")),
                    path=path,
                )
    return objects, rects


def read_meta_guid(script_path: Path) -> str:
    meta_path = script_path.with_suffix(script_path.suffix + ".meta")
    if not meta_path.exists():
        return ""
    match = re.search(r"^guid:\s*([0-9a-fA-F]+)\s*$", meta_path.read_text(errors="replace"), re.MULTILINE)
    return match.group(1) if match else ""


def is_attachable_script(script_path: Path) -> bool:
    text = script_path.read_text(errors="replace")
    return bool(re.search(r":\s*MonoBehaviour\b", text))


def scan_script_usage(yaml_files: list[Path], script_files: list[Path]) -> list[ScriptUsageInfo]:
    guid_refs: Counter[str] = Counter()
    script_ref_re = re.compile(r"m_Script:\s*\{fileID:\s*11500000,\s*guid:\s*([^,}]+)")
    for path in yaml_files:
        for guid in script_ref_re.findall(path.read_text(errors="replace")):
            guid_refs[guid.strip()] += 1

    usage: list[ScriptUsageInfo] = []
    for script_path in script_files:
        if not is_attachable_script(script_path):
            continue
        guid = read_meta_guid(script_path)
        if not guid:
            continue
        usage.append(ScriptUsageInfo(guid=guid, path=script_path, reference_count=guid_refs[guid]))
    usage.sort(key=lambda item: (item.reference_count, str(item.path)))
    return usage


def point_anchor(lines: list[str], idx: int) -> bool:
    """Return true if adjacent AnchorMin/AnchorMax lines pin to one point."""
    line = lines[idx]
    match = PATTERNS["anchor"].search(line)
    if not match:
        return False
    current = match.groups()
    for j in range(idx + 1, min(idx + 4, len(lines))):
        other = PATTERNS["anchor"].search(lines[j])
        if other and other.groups() == current:
            return True
    return False


def scan_yaml(files: list[Path]) -> tuple[Counter[str], dict[str, list[str]]]:
    counts: Counter[str] = Counter()
    samples: dict[str, list[str]] = defaultdict(list)
    for path in files:
        rel = str(path.relative_to(ROOT))
        lines = path.read_text(errors="replace").splitlines()
        for i, line in enumerate(lines, start=1):
            for name in ("canvas_scaler", "size_delta", "anchored_position"):
                if PATTERNS[name].search(line):
                    counts[name] += 1
                    if len(samples[name]) < 12:
                        samples[name].append(f"{rel}:{i}: {line.strip()}")
            if point_anchor(lines, i - 1):
                counts["point_anchor"] += 1
                if len(samples["point_anchor"]) < 12:
                    samples["point_anchor"].append(f"{rel}:{i}: {line.strip()}")
    return counts, samples


def scan_scripts(files: list[Path]) -> tuple[Counter[str], dict[str, list[str]]]:
    counts: Counter[str] = Counter()
    samples: dict[str, list[str]] = defaultdict(list)
    for path in files:
        rel = str(path.relative_to(ROOT))
        lines = path.read_text(errors="replace").splitlines()
        for i, line in enumerate(lines, start=1):
            for name in (
                "runtime_size",
                "runtime_position",
                "layout_rebuild",
                "screen_dependency",
            ):
                if PATTERNS[name].search(line):
                    counts[name] += 1
                    if len(samples[name]) < 16:
                        samples[name].append(f"{rel}:{i}: {line.strip()}")
    return counts, samples


def print_section(title: str, counts: Counter[str], samples: dict[str, list[str]]) -> None:
    print(f"\n## {title}")
    for key, value in counts.most_common():
        print(f"- {key}: {value}")
    for key in sorted(samples):
        print(f"\n### {key} samples")
        for sample in samples[key]:
            print(f"- {sample}")


def print_named_objects(pattern: str, files: list[Path]) -> None:
    objects, rects = parse_yaml_objects(files)
    object_rects = {rect.game_object_id: rect for rect in rects.values()}
    needle = re.compile(pattern, re.IGNORECASE)
    matches = [obj for obj in objects.values() if needle.search(obj.name)]
    matches.sort(key=lambda item: (str(item.path), item.name, item.file_id))

    print(f"\n## Named objects matching /{pattern}/")
    if not matches:
        print("- no matches")
        return

    for obj in matches:
        rel = obj.path.relative_to(ROOT)
        rect = object_rects.get(obj.file_id)
        print(f"\n### {obj.name} ({rel}, fileID {obj.file_id})")
        print(f"- components: {', '.join(obj.components) if obj.components else 'none'}")
        if rect is None:
            print("- RectTransform: none")
            continue
        father = rect.father if rect.father else "0"
        print(f"- RectTransform fileID: {rect.file_id}, father: {father}")
        print(f"- anchorMin: {rect.anchor_min}")
        print(f"- anchorMax: {rect.anchor_max}")
        print(f"- anchoredPosition: {rect.anchored_position}")
        print(f"- sizeDelta: {rect.size_delta}")
        print(f"- pivot: {rect.pivot}")


def print_script_usage(yaml_files: list[Path], script_files: list[Path]) -> None:
    usage = scan_script_usage(yaml_files, script_files)
    unreferenced = [item for item in usage if item.reference_count == 0]

    print("\n## Script usage")
    print(f"- attachable_scripts={len(usage)}")
    print(f"- unreferenced_attachable_scripts={len(unreferenced)}")
    if unreferenced:
        print("\n### unreferenced attachable scripts")
        for item in unreferenced[:40]:
            rel = item.path.relative_to(ROOT)
            print(f"- {rel} (guid {item.guid})")


def scan_horb_risk(files: list[Path]) -> tuple[Counter[str], dict[str, list[str]]]:
    counts: Counter[str] = Counter()
    samples: dict[str, list[str]] = defaultdict(list)
    sample_keys = {
        "manual_horb_json",
        "plain_text_format",
        "plain_text_multiline_literal",
        "rich_no_scroll",
    }
    patterns = {
        "horb_new": re.compile(r"\bHorb::new\s*\("),
        "plain_text": re.compile(r"\.text\s*\("),
        "plain_text_format": re.compile(r"\.text\s*\(\s*format!\s*\("),
        "list_row": re.compile(r"\.list_row\s*\("),
        "rich_row": re.compile(r"\.rich_row\s*\("),
        "button": re.compile(r"\.button\s*\("),
        "minimap": re.compile(r"\.minimap\s*\("),
        "inventory": re.compile(r"\.inventory\s*\("),
        "card": re.compile(r"\.card\s*\("),
        "input": re.compile(r"\.input\s*\("),
        "manual_horb_json": re.compile(r'(?:b|format!)?\s*\(\s*"horb:|"horb:'),
        "rich_no_scroll": re.compile(r"rich_no_scroll"),
    }
    for path in files:
        rel = str(path.relative_to(ROOT))
        lines = path.read_text(errors="replace").splitlines()
        for i, line in enumerate(lines, start=1):
            stripped = line.strip()
            for key, pattern in patterns.items():
                if key == "manual_horb_json" and (
                    stripped.startswith("//") or stripped.startswith("assert!")
                ):
                    continue
                if not pattern.search(line):
                    continue
                counts[key] += 1
                if key in sample_keys and len(samples[key]) < 20:
                    samples[key].append(f"{rel}:{i}: {line.strip()}")
            if ".text(" in line and len(line) > 120:
                counts["plain_text_long_line"] += 1
                if len(samples["plain_text_long_line"]) < 20:
                    samples["plain_text_long_line"].append(f"{rel}:{i}: {line.strip()}")
            if ".text(" in line and "\\n" in line:
                counts["plain_text_multiline_literal"] += 1
                if len(samples["plain_text_multiline_literal"]) < 20:
                    samples["plain_text_multiline_literal"].append(f"{rel}:{i}: {line.strip()}")
    return counts, samples


def print_horb_risk() -> None:
    counts, samples = scan_horb_risk(iter_root_files(SERVER_HORB_GLOBS))
    print("\n## Server HORB risk")
    for key in (
        "horb_new",
        "plain_text",
        "plain_text_format",
        "plain_text_long_line",
        "plain_text_multiline_literal",
        "list_row",
        "rich_row",
        "button",
        "minimap",
        "inventory",
        "card",
        "input",
        "manual_horb_json",
        "rich_no_scroll",
    ):
        print(f"- {key}: {counts.get(key, 0)}")
    for key in sorted(samples):
        print(f"\n### {key} samples")
        for sample in samples[key]:
            print(f"- {sample}")


def density_boost_for(
    width: int,
    height: int,
    dpi: int,
    *,
    mobile: bool = False,
    macbook_fallback: bool = False,
) -> float:
    if dpi <= 0 and macbook_fallback:
        if min(width, height) > MAC_RETINA_FALLBACK_MAX_SHORT_SIDE:
            return 1.0
        dpi = min(width, height) / MAC_RETINA_FALLBACK_SHORT_SIDE_INCHES
    if dpi <= 0:
        return 1.0
    short_side_inches = min(width, height) / dpi
    if short_side_inches <= 0.1:
        return 1.0
    max_boost = DENSITY_BOOST_MAX if mobile else DESKTOP_DENSITY_BOOST_MAX
    return max(
        1.0,
        min(
            (REFERENCE_SCREEN_INCHES / short_side_inches) ** DENSITY_BOOST_EXPONENT,
            max_boost,
        ),
    )


def resolution_scale_for(width: int, height: int) -> float:
    short_side = max(min(width, height), 1)
    return max(0.5, min(short_side / DESIGN_HEIGHT, 4.0))


def scale_row(
    width: int,
    height: int,
    dpi: int,
    *,
    mobile: bool = False,
    macbook_fallback: bool = False,
) -> ScaleRow:
    density = density_boost_for(
        width,
        height,
        dpi,
        mobile=mobile,
        macbook_fallback=macbook_fallback,
    )
    resolution_scale = resolution_scale_for(width, height)
    scale = UI_SIZE * density
    if width < height:
        reference_width = DESIGN_HEIGHT / scale
        reference_height = DESIGN_WIDTH / scale
    else:
        reference_width = DESIGN_WIDTH / scale
        reference_height = DESIGN_HEIGHT / scale
    world_tile_pixels = max(
        TERRAIN_TILE_SIZE * WORLD_ZOOM_BASE * (density**0.5),
        MIN_WORLD_TILE_PIXELS,
    ) * resolution_scale
    return ScaleRow(
        width=width,
        height=height,
        dpi=dpi,
        density=density,
        reference_width=reference_width,
        reference_height=reference_height,
        resolution_scale=resolution_scale,
        world_tile_pixels=world_tile_pixels,
    )


def print_scale_matrix() -> None:
    print("\n## DisplayScale matrix")
    print(
        "Formula mirror for desktop builds: DensityBoost uses Screen.dpi but is capped "
        f"at {DESKTOP_DENSITY_BOOST_MAX}; macOS dpi=0 fallback uses "
        f"~{MAC_RETINA_FALLBACK_SHORT_SIDE_INCHES:.2f}\" short-side physical size when "
        f"screen short side <= {MAC_RETINA_FALLBACK_MAX_SHORT_SIDE}; mobile cap remains "
        f"{DENSITY_BOOST_MAX}."
    )
    print(
        "| Resolution | DPI | DensityBoost | Canvas reference | ResScale | World tile px |"
    )
    print("| --- | ---: | ---: | ---: | ---: | ---: |")
    for width, height in DEFAULT_SCALE_RESOLUTIONS:
        baseline = scale_row(width, height, 0)
        for dpi in DEFAULT_SCALE_DPI:
            row = scale_row(width, height, dpi)
            marker = ""
            if dpi > 0 and row.density >= DENSITY_BOOST_MAX:
                marker = " cap"
            elif dpi > 0 and row.density >= 1.5:
                marker = " high"
            print(
                f"| {width}x{height} | {dpi} | {row.density:.2f}{marker} | "
                f"{row.reference_width:.0f}x{row.reference_height:.0f} | "
                f"{row.resolution_scale:.2f} | {row.world_tile_pixels:.1f} |"
            )
        mac = scale_row(width, height, 0, macbook_fallback=True)
        print(
            f"| {width}x{height} | macbook dpi=0 | {mac.density:.2f} fallback | "
            f"{mac.reference_width:.0f}x{mac.reference_height:.0f} | "
            f"{mac.resolution_scale:.2f} | {mac.world_tile_pixels:.1f} |"
        )
        worst = scale_row(width, height, max(DEFAULT_SCALE_DPI))
        hud_growth = worst.density / baseline.density
        world_growth = worst.world_tile_pixels / baseline.world_tile_pixels
        if hud_growth >= 1.5:
            print(
                f"| {width}x{height} | risk | HUD x{hud_growth:.2f}; "
                f"world tile x{world_growth:.2f} vs dpi=0 |  |  |  |"
            )


def rect_point_anchored(rect: RectTransformInfo) -> bool:
    return rect.anchor_min == rect.anchor_max


def rect_size_delta(rect: RectTransformInfo) -> tuple[float, float]:
    match = re.match(r"\{x:\s*([^,]+), y:\s*([^}]+)\}", rect.size_delta)
    if not match:
        return 0.0, 0.0
    return float(match.group(1)), float(match.group(2))


def print_fit_matrix(pattern: str) -> None:
    yaml_files = iter_files(YAML_GLOBS)
    objects, rects = parse_yaml_objects(yaml_files)
    needle = re.compile(pattern, re.IGNORECASE)
    targets = [obj for obj in objects.values() if needle.search(obj.name)]
    targets.sort(key=lambda item: (str(item.path), item.name, item.file_id))
    if not targets:
        print(f"\n## Fit matrix /{pattern}/")
        print("- no matches")
        return

    object_rects = {rect.game_object_id: rect for rect in rects.values()}
    print(f"\n## Fit matrix /{pattern}/")
    print(
        "Assumption: point-anchored windows scale uniformly with CanvasScaler "
        "using the current DisplayScale reference resolution."
    )
    print(
        "| Object | Resolution | DPI | Scale | Size px | Margin px |"
    )
    print("| --- | --- | ---: | ---: | ---: | ---: |")
    for obj in targets:
        rect = object_rects.get(obj.file_id)
        if rect is None:
            print(f"| {obj.name} | missing RectTransform |  |  |  |")
            continue
        if not rect_point_anchored(rect):
            print(f"| {obj.name} | non-point anchor, skipped |  |  |  |")
            continue
        size_x, size_y = rect_size_delta(rect)
        if size_x == 0.0 and size_y == 0.0:
            print(f"| {obj.name} | invalid sizeDelta |  |  |  |")
            continue

        for width, height in DEFAULT_SCALE_RESOLUTIONS:
            fit_rows = [
                (str(dpi), scale_row(width, height, dpi))
                for dpi in (0, 110, 160, 220)
            ]
            fit_rows.append(
                (
                    "macbook dpi=0",
                    scale_row(width, height, 0, macbook_fallback=True),
                )
            )
            for dpi_label, row in fit_rows:
                canvas_scale = (
                    width / row.reference_width
                    if width < height
                    else height / row.reference_height
                )
                pixel_width = abs(size_x) * canvas_scale
                pixel_height = abs(size_y) * canvas_scale
                margin_x = (width - pixel_width) / 2.0
                margin_y = (height - pixel_height) / 2.0
                risk = ""
                if margin_x < 24 or margin_y < 24:
                    risk = " risk"
                print(
                    f"| {obj.name}{risk} | {width}x{height} | {dpi_label} | "
                    f"{canvas_scale:.2f} | {pixel_width:.0f}x{pixel_height:.0f} | "
                    f"{margin_x:.0f}x{margin_y:.0f} |"
                )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--samples-only",
        action="store_true",
        help="omit aggregate counts and print samples only",
    )
    parser.add_argument(
        "--object",
        action="append",
        default=[],
        metavar="REGEX",
        help="print RectTransform details for GameObjects whose names match REGEX",
    )
    parser.add_argument(
        "--script-usage",
        action="store_true",
        help="print MonoBehaviour script asset usage across scenes and prefabs",
    )
    parser.add_argument(
        "--scale-matrix",
        action="store_true",
        help="print DisplayScale DPI/resolution impact matrix",
    )
    parser.add_argument(
        "--fit-matrix",
        action="append",
        default=[],
        metavar="REGEX",
        help="print approximate pixel fit matrix for point-anchored scene windows",
    )
    parser.add_argument(
        "--matrix-only",
        action="store_true",
        help="omit the default aggregate audit and print only requested detail sections",
    )
    parser.add_argument(
        "--horb-risk",
        action="store_true",
        help="print server HORB builder usage and dynamic-content risk counters",
    )
    args = parser.parse_args()

    yaml_files = iter_files(YAML_GLOBS)
    script_files = iter_files(SCRIPT_GLOBS)

    if not args.matrix_only:
        yaml_counts, yaml_samples = scan_yaml(yaml_files)
        script_counts, script_samples = scan_scripts(script_files)

        print("# Unity UI layout audit")
        print(f"yaml_files={len(yaml_files)} script_files={len(script_files)}")
        if args.samples_only:
            print_section("YAML", Counter(), yaml_samples)
            print_section("Scripts", Counter(), script_samples)
        else:
            print_section("YAML", yaml_counts, yaml_samples)
            print_section("Scripts", script_counts, script_samples)
    else:
        print("# Unity UI detail audit")
    if args.script_usage:
        print_script_usage(yaml_files, script_files)
    if args.horb_risk:
        print_horb_risk()
    if args.scale_matrix:
        print_scale_matrix()
    for pattern in args.fit_matrix:
        print_fit_matrix(pattern)
    for pattern in args.object:
        print_named_objects(pattern, yaml_files)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
