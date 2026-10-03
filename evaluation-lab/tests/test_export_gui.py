"""Synthetic rendered-HUD validation controls; actual GUI evidence is separate."""
import copy
from pathlib import Path
import sys
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"native"))
from export_gui import parse_status, assess


class ExportGuiTests(unittest.TestCase):
    def setUp(self):
        self.spec = {"objective_count":4,"timer_seconds":60}
        self.rows = [parse_status(text) for text in (
            "Delivered 0/4 | 59.8s", "Delivered 0/4 | 59.1s", "Delivered 1/4 | 58.0s",
            "Delivered 4/4 | 57.0s | Complete", "Delivered 0/4 | 59.7s")]

    def test_complete_keyboard_sequence(self):
        self.assertTrue(all(assess(self.rows,self.spec).values()))

    def test_absent_or_ambiguous_ocr_refused(self):
        for text in ("", "Delivered ?/4 | 59.1s", "Delivered 0/4 | 59.1s Delivered 0/4 | 59.1s"):
            with self.assertRaises(ValueError): parse_status(text)

    def test_inconsistent_rendered_counts_refused(self):
        for text in ("Delivered 5/4 | 59.1s", "Delivered 0/0 | 59.1s"):
            with self.assertRaises(ValueError): parse_status(text)

    def test_noop_keyboard_cannot_pass_completion(self):
        rows = copy.deepcopy(self.rows); rows[2]["count"] = 0; rows[3].update(count=0,complete=False)
        checks = assess(rows,self.spec)
        self.assertFalse(checks["keyboard_movement_and_pickup"])
        self.assertFalse(checks["keyboard_objective_completion"])

    def test_wrong_goal_and_false_complete_refused(self):
        rows = copy.deepcopy(self.rows); rows[3].update(goal=8,complete=False)
        checks = assess(rows,self.spec)
        self.assertFalse(checks["rendered_objective"])
        self.assertFalse(checks["keyboard_objective_completion"])

    def test_frozen_timer_and_broken_restart_refused(self):
        rows = copy.deepcopy(self.rows); rows[1]["remaining_seconds"] = rows[0]["remaining_seconds"]
        rows[-1].update(count=4,complete=True,remaining_seconds=57)
        checks = assess(rows,self.spec)
        self.assertFalse(checks["rendered_timer_active"])
        self.assertFalse(checks["keyboard_restart"])
        self.assertFalse(checks["rendered_timer_reset"])
