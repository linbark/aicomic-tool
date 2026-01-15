import time
import unittest
import sys
from pathlib import Path

_BACKEND_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(_BACKEND_DIR))

from app.services.llm_client import parse_question_answer_json


class TestParseQuestionAnswerJson(unittest.TestCase):
    def test_valid_json(self):
        out = parse_question_answer_json(
            question="你好？",
            content='{"question":"你好？","answer":"你好！"}',
        )
        self.assertEqual(out["question"], "你好？")
        self.assertEqual(out["answer"], "你好！")

    def test_missing_question_fills_from_input(self):
        out = parse_question_answer_json(
            question="Q1",
            content='{"answer":"A1"}',
        )
        self.assertEqual(out["question"], "Q1")
        self.assertEqual(out["answer"], "A1")

    def test_wrapped_json_extracts(self):
        out = parse_question_answer_json(
            question="Q2",
            content='前缀 {"answer":"A2","question":"Q2"} 后缀',
        )
        self.assertEqual(out["question"], "Q2")
        self.assertEqual(out["answer"], "A2")

    def test_missing_answer_raises(self):
        with self.assertRaises(ValueError):
            parse_question_answer_json(question="Q3", content='{"question":"Q3"}')

    def test_empty_answer_raises(self):
        with self.assertRaises(ValueError):
            parse_question_answer_json(question="Q4", content='{"question":"Q4","answer":"  "}')


class TestParsePerformance(unittest.TestCase):
    def test_benchmark(self):
        content = '{"question":"Q","answer":"A"}'
        loops = 20000
        t0 = time.perf_counter()
        for _ in range(loops):
            parse_question_answer_json(question="Q", content=content)
        dt = time.perf_counter() - t0
        self.assertLess(dt, 1.5)


if __name__ == "__main__":
    unittest.main()
