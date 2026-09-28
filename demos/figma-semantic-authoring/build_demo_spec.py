#!/usr/bin/env python3
"""Generate the deterministic Semwright landing composition fixture."""
from __future__ import annotations
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent

def rgb(value: str, alpha: float = 1.0) -> dict:
    value = value.lstrip("#")
    return {
        "kind": "solid",
        "r": int(value[0:2], 16) / 255,
        "g": int(value[2:4], 16) / 255,
        "b": int(value[4:6], 16) / 255,
        "a": alpha,
    }

IVORY = rgb("#F4EFE6")
PAPER = rgb("#FBF8F2")
INK = rgb("#102A56")
COBALT = rgb("#2455D6")
MUTED = rgb("#657086")
HAIRLINE = rgb("#D7D0C5")

def axis(mode: str, value: float | None = None) -> dict:
    return {"mode": mode, "value": value, "min": None, "max": None}

def sizing(width: tuple[str, float | None], height: tuple[str, float | None], ratio=None) -> dict:
    return {"width": axis(*width), "height": axis(*height), "aspect_ratio": ratio}

def layout(direction: str, gap: float = 0, padding=(0, 0, 0, 0),
           align="start", distribute="start", wrap=False) -> dict:
    top, right, bottom, left = padding
    return {
        "direction": direction, "gap": gap,
        "padding": {"top": top, "right": right, "bottom": bottom, "left": left},
        "align": align, "distribute": distribute,
        "wrap": wrap, "absolute_children": False,
    }

def frame(id_, name, parent, order, *, direction="vertical", gap=0,
          pad=(0, 0, 0, 0), width=("fill", None), height=("fixed", 100),
          fill=None, radius=None, role=None, align="start", distribute="start"):
    node = {
        "id": id_, "kind": "stack" if direction == "vertical" else "row",
        "name": name, "parent": parent, "order": order, "role": role,
        "layout": layout(direction, gap, pad, align, distribute),
        "sizing": sizing(width, height),
    }
    if fill is not None or radius is not None:
        node["visual"] = {"fill": fill, "radius": radius, "opacity": 1, "text_style": None}
    return node

def text(id_, name, parent, order, copy, size, line, *, width=("fill", None),
         height=("hug", None), role=None, color=INK, max_lines=None):
    return {
        "id": id_, "kind": "text", "name": name, "parent": parent, "order": order,
        "role": role, "sizing": sizing(width, height),
        "text": {
            "characters": copy, "font_family": "Inter", "font_style": "Regular",
            "font_size": size, "line_height": line, "letter_spacing": 0,
            "max_lines": max_lines, "fit": "grow_height",
        },
        "visual": {"fill": color, "radius": None, "opacity": 1, "text_style": None},
    }

def shape(id_, name, parent, order, width, height, fill, radius=0, role=None):
    return {
        "id": id_, "kind": "shape", "name": name, "parent": parent, "order": order,
        "role": role, "sizing": sizing(("fixed", width), ("fixed", height)),
        "visual": {"fill": fill, "radius": radius, "opacity": 1, "text_style": None},
    }

nodes = []

# Desktop: editorial, evidence-led, intentionally one spacing defect for bounded repair.
nodes += [
    frame("desktop", "Semwright / Launch / Desktop", None, 0, gap=0,
          width=("fixed", 1440), height=("fixed", 4100), fill=IVORY, role="page"),
    frame("d-nav", "01 / Navigation", "desktop", 0, direction="horizontal", gap=24,
          pad=(32, 72, 32, 72), height=("fixed", 104), align="center",
          distribute="space_between", role="navigation"),
    text("d-brand", "Wordmark", "d-nav", 0, "semwright", 28, 32, width=("hug", None), role="brand"),
    text("d-nav-note", "Navigation note", "d-nav", 1,
         "semantic execution / native evidence", 14, 20, width=("hug", None), color=MUTED),
    frame("d-hero", "02 / Hero", "desktop", 1, direction="horizontal", gap=72,
          pad=(88, 72, 88, 72), height=("fixed", 880), align="center", role="hero"),
    frame("d-hero-copy", "Hero / Editorial copy", "d-hero", 0, gap=18,
          width=("fixed", 620), height=("fixed", 620), role="hero-copy"),
    text("d-kicker", "Hero / Kicker", "d-hero-copy", 0, "NATIVE SEMANTIC EXECUTION", 14, 20,
         width=("fixed", 520), color=COBALT, role="eyebrow"),
    text("d-heading", "Hero / Heading", "d-hero-copy", 1,
         "Agents should spend intelligence on judgment, not coordinates.", 72, 78,
         width=("fixed", 610), max_lines=4, role="heading"),
    text("d-body", "Hero / Body", "d-hero-copy", 2,
         "Semwright turns structured intent into native Figma objects, measures what actually happened, "
         "and converges only through bounded, policy-authorized changes.", 21, 31,
         width=("fixed", 570), role="body", color=MUTED),
    frame("d-cta", "Hero / CTA", "d-hero-copy", 3, direction="horizontal", gap=12,
          pad=(18, 24, 18, 24), width=("fixed", 310), height=("fixed", 64),
          fill=COBALT, radius=8, align="center", role="cta"),
    text("d-cta-label", "CTA / Label", "d-cta", 0, "Inspect the execution model →", 16, 20,
         width=("hug", None), color=PAPER, role="label"),
    frame("d-proof", "Hero / Evidence field", "d-hero", 1, gap=20,
          pad=(32, 32, 32, 32), width=("fixed", 604), height=("fixed", 620),
          fill=INK, radius=12, role="evidence"),
    text("d-proof-kicker", "Evidence / Kicker", "d-proof", 0, "OBSERVED, NOT ASSUMED", 13, 18,
         width=("fixed", 500), color=rgb("#9DB7FF"), role="eyebrow"),
    text("d-proof-title", "Evidence / Title", "d-proof", 1,
         "Intent → plan → native build → measure → validate → repair → verify", 32, 39,
         width=("fixed", 520), color=PAPER, max_lines=4, role="heading"),
    frame("d-proof-lines", "Evidence / Contract rows", "d-proof", 2, gap=12,
          width=("fill", None), height=("fixed", 280), role="evidence-list"),
    text("d-proof-1", "Evidence / 01", "d-proof-lines", 0,
         "01  Text remains TextNode content", 16, 23, color=PAPER),
    text("d-proof-2", "Evidence / 02", "d-proof-lines", 1,
         "02  Auto Layout owns mechanical spacing", 16, 23, color=PAPER),
    text("d-proof-3", "Evidence / 03", "d-proof-lines", 2,
         "03  Revisions invalidate stale plans", 16, 23, color=PAPER),
    text("d-proof-4", "Evidence / 04", "d-proof-lines", 3,
         "04  Visual review is evidence, never object identity", 16, 23, color=PAPER),
    frame("d-thesis", "03 / Thesis", "desktop", 2, gap=28,
          pad=(104, 160, 104, 160), height=("fixed", 620), fill=PAPER, role="section"),
    text("d-thesis-num", "Thesis / Number", "d-thesis", 0, "03 / A different execution boundary", 14, 20,
         width=("fixed", 820), color=COBALT),
    text("d-thesis-head", "Thesis / Heading", "d-thesis", 1,
         "The model decides what matters. Semwright makes the mechanics inspectable.", 52, 60,
         width=("fixed", 940), max_lines=3, role="heading"),
    text("d-thesis-body", "Thesis / Body", "d-thesis", 2,
         "No hidden JavaScript. No coordinate theater. No screenshot pretending to be a document. "
         "The output is still Figma: editable, structured, and addressable by native identity.", 20, 30,
         width=("fixed", 820), color=MUTED),
    frame("d-flow", "04 / Authority flow", "desktop", 3, direction="horizontal", gap=24,
          pad=(112, 72, 112, 72), height=("fixed", 620), align="center",
          distribute="space_between", role="architecture"),
    frame("d-agent", "Flow / Agent", "d-flow", 0, gap=10, pad=(28, 28, 28, 28),
          width=("fixed", 300), height=("fixed", 220), fill=PAPER, radius=8),
    text("d-agent-label", "Agent / Label", "d-agent", 0, "AGENT", 13, 18, color=COBALT),
    text("d-agent-copy", "Agent / Copy", "d-agent", 1, "taste\nintent\njudgment", 28, 36,
         width=("fixed", 230)),
    text("d-arrow-1", "Flow / Arrow 1", "d-flow", 1, "→", 36, 40, width=("hug", None), color=MUTED),
    frame("d-sw", "Flow / Semwright", "d-flow", 2, gap=10, pad=(28, 28, 28, 28),
          width=("fixed", 360), height=("fixed", 270), fill=INK, radius=8),
    text("d-sw-label", "Semwright / Label", "d-sw", 0, "SEMWRIGHT", 13, 18, color=rgb("#9DB7FF")),
    text("d-sw-copy", "Semwright / Copy", "d-sw", 1,
         "refs\npolicy\nconstraints\nmeasurement\nevidence", 24, 32,
         width=("fixed", 270), color=PAPER),
    text("d-arrow-2", "Flow / Arrow 2", "d-flow", 3, "→", 36, 40, width=("hug", None), color=MUTED),
    frame("d-figma", "Flow / Figma", "d-flow", 4, gap=10, pad=(28, 28, 28, 28),
          width=("fixed", 300), height=("fixed", 220), fill=PAPER, radius=8),
    text("d-figma-label", "Figma / Label", "d-figma", 0, "FIGMA", 13, 18, color=COBALT),
    text("d-figma-copy", "Figma / Copy", "d-figma", 1, "native nodes\neditable structure\nofficial APIs", 24, 32,
         width=("fixed", 230)),
    frame("d-verify", "05 / Verification", "desktop", 4, direction="horizontal", gap=80,
          pad=(112, 96, 112, 96), height=("fixed", 760), fill=INK, align="center", role="verification"),
    frame("d-verify-copy", "Verification / Copy", "d-verify", 0, gap=20,
          width=("fixed", 560), height=("fixed", 460)),
    text("d-verify-kicker", "Verification / Kicker", "d-verify-copy", 0, "PASS / FAIL / UNKNOWN", 14, 20,
         color=rgb("#9DB7FF")),
    text("d-verify-head", "Verification / Heading", "d-verify-copy", 1,
         "A screenshot can support judgment. It cannot replace semantic state.", 46, 54,
         width=("fixed", 540), color=PAPER, max_lines=4),
    text("d-verify-body", "Verification / Body", "d-verify-copy", 2,
         "Deterministic findings can fail. Heuristics stay labeled. Aesthetic review stays with the model or human.",
         19, 29, width=("fixed", 500), color=rgb("#CCD5E8")),
    frame("d-status", "Verification / Status ledger", "d-verify", 1, gap=18,
          pad=(32, 32, 32, 32), width=("fixed", 560), height=("fixed", 430),
          fill=rgb("#17386F"), radius=10),
    text("d-status-1", "Status / Deterministic", "d-status", 0, "DETERMINISTIC     PASS / FAIL", 17, 24, color=PAPER),
    text("d-status-2", "Status / Heuristic", "d-status", 1, "HEURISTIC         advisory", 17, 24, color=PAPER),
    text("d-status-3", "Status / Aesthetic", "d-status", 2, "AESTHETIC_ASSIST  review artifact", 17, 24, color=PAPER),
    text("d-status-4", "Status / Revision", "d-status", 3, "REVISION           bound before apply", 17, 24, color=PAPER),
    frame("d-close", "06 / Closing", "desktop", 5, gap=24,
          pad=(120, 160, 120, 160), height=("fixed", 760), role="closing"),
    text("d-close-kicker", "Closing / Kicker", "d-close", 0, "SEMANTICS BEFORE MECHANICS", 14, 20,
         width=("fixed", 720), color=COBALT),
    text("d-close-head", "Closing / Heading", "d-close", 1,
         "More capable models should make Semwright more useful, not less necessary.", 58, 66,
         width=("fixed", 980), max_lines=3),
    text("d-close-body", "Closing / Body", "d-close", 2,
         "Intelligence raises the creative ceiling. A typed execution layer raises the correctness floor.",
         21, 31, width=("fixed", 760), color=MUTED),
]

# Mobile: same semantics, deliberately re-authored as a stack rather than fake CSS responsiveness.
nodes += [
    frame("mobile", "Semwright / Launch / Mobile", None, 1, gap=0,
          width=("fixed", 390), height=("fixed", 3300), fill=IVORY, role="page"),
    frame("m-nav", "M / Navigation", "mobile", 0, direction="horizontal", gap=12,
          pad=(24, 20, 24, 20), height=("fixed", 84), align="center",
          distribute="space_between", role="navigation"),
    text("m-brand", "M / Wordmark", "m-nav", 0, "semwright", 24, 28, width=("hug", None), role="brand"),
    text("m-index", "M / Index", "m-nav", 1, "01—06", 12, 16, width=("hug", None), color=MUTED),
    frame("m-hero", "M / Hero", "mobile", 1, gap=24, pad=(56, 20, 56, 20),
          height=("fixed", 840), role="hero"),
    text("m-kicker", "M Hero / Kicker", "m-hero", 0, "NATIVE SEMANTIC EXECUTION", 12, 18,
         width=("fixed", 330), color=COBALT),
    text("m-heading", "M Hero / Heading", "m-hero", 1,
         "Meaning before mechanics.", 46, 50, width=("fixed", 340), max_lines=3, role="heading"),
    text("m-body", "M Hero / Body", "m-hero", 2,
         "Structured intent becomes native Figma, then measured state decides what happens next.",
         18, 27, width=("fixed", 340), color=MUTED, role="body"),
    frame("m-cta", "M Hero / CTA", "m-hero", 3, direction="horizontal", gap=8,
          pad=(16, 18, 16, 18), width=("fixed", 350), height=("fixed", 58),
          fill=COBALT, radius=8, align="center", role="cta"),
    text("m-cta-label", "M CTA / Label", "m-cta", 0, "Inspect the model →", 15, 20,
         width=("hug", None), color=PAPER),
    frame("m-proof", "M / Evidence", "mobile", 2, gap=18, pad=(32, 20, 32, 20),
          height=("fixed", 610), fill=INK, role="evidence"),
    text("m-proof-kicker", "M Evidence / Kicker", "m-proof", 0, "OBSERVED, NOT ASSUMED", 12, 18,
         width=("fixed", 340), color=rgb("#9DB7FF")),
    text("m-proof-head", "M Evidence / Heading", "m-proof", 1,
         "Plan. Build. Measure. Validate. Repair. Verify.", 32, 38,
         width=("fixed", 340), color=PAPER, max_lines=4),
    text("m-proof-1", "M Evidence / 01", "m-proof", 2, "Native text stays editable.", 16, 23,
         width=("fixed", 340), color=PAPER),
    text("m-proof-2", "M Evidence / 02", "m-proof", 3, "Stale plans stop instead of replaying.", 16, 23,
         width=("fixed", 340), color=PAPER),
    text("m-proof-3", "M Evidence / 03", "m-proof", 4, "Visual judgment stays explicitly non-deterministic.", 16, 23,
         width=("fixed", 340), color=PAPER),
    frame("m-thesis", "M / Thesis", "mobile", 3, gap=24, pad=(64, 20, 64, 20),
          height=("fixed", 630), fill=PAPER, role="section"),
    text("m-thesis-num", "M Thesis / Number", "m-thesis", 0, "03 / EXECUTION BOUNDARY", 12, 18,
         width=("fixed", 340), color=COBALT),
    text("m-thesis-head", "M Thesis / Heading", "m-thesis", 1,
         "The model judges. The runtime proves.", 38, 44, width=("fixed", 340), max_lines=3),
    text("m-thesis-body", "M Thesis / Body", "m-thesis", 2,
         "Refs, policy, constraints and evidence stay below the prompt layer.",
         18, 27, width=("fixed", 340), color=MUTED),
    frame("m-close", "M / Closing", "mobile", 4, gap=24, pad=(72, 20, 72, 20),
          height=("fixed", 760), role="closing"),
    text("m-close-kicker", "M Closing / Kicker", "m-close", 0, "SEMANTICS BEFORE MECHANICS", 12, 18,
         width=("fixed", 340), color=COBALT),
    text("m-close-head", "M Closing / Heading", "m-close", 1,
         "Better models should widen the design space, not reopen the authority boundary.",
         38, 44, width=("fixed", 340), max_lines=5),
    text("m-close-body", "M Closing / Body", "m-close", 2,
         "The artifact is editable. The evidence is inspectable. The mutation remains policy-controlled.",
         18, 27, width=("fixed", 340), color=MUTED),
]

spec = {
    "version": 1,
    "target": {"page_id": None, "parent_node_id": None},
    "nodes": nodes,
    "relationships": [
        {
            "kind": "minimum_gap", "subject": "d-heading", "object": "d-body",
            "value": 24, "tolerance": 0.5,
        },
        {
            "kind": "minimum_gap", "subject": "m-heading", "object": "m-body",
            "value": 24, "tolerance": 0.5,
        },
    ],
    "profiles": [
        {"name": "desktop", "width": 1440, "root_id": "desktop"},
        {"name": "mobile", "width": 390, "root_id": "mobile"},
    ],
    "validators": [
        {"kind": "text_clipping", "severity": "error"},
        {"kind": "parent_overflow", "severity": "error"},
        {"kind": "sibling_overlap", "severity": "warning"},
        {"kind": "declared_spacing", "severity": "warning"},
        {"kind": "alignment", "severity": "warning"},
        {"kind": "aspect_ratio", "severity": "warning"},
        {"kind": "touch_target", "severity": "warning"},
        {"kind": "contrast", "severity": "warning"},
        {"kind": "fonts", "severity": "error"},
        {"kind": "auto_layout", "severity": "error"},
        {"kind": "responsive_profile", "severity": "error"},
        {"kind": "prototype_refs", "severity": "error"},
        {"kind": "required_bindings", "severity": "error"},
        {"kind": "hidden_overflow", "severity": "warning"},
        {"kind": "native_text", "severity": "error"},
    ],
    "budgets": {
        "max_nodes": 160,
        "max_depth": 12,
        "max_relationships": 64,
        "max_findings_per_round": 128,
        "max_repair_operations": 16,
        "max_iterations": 4,
        "max_mutations": 48,
    },
}

if __name__ == "__main__":
    output = HERE / "semwright-landing.composition.json"
    output.write_text(json.dumps(spec, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Wrote {output} ({len(nodes)} nodes)")

