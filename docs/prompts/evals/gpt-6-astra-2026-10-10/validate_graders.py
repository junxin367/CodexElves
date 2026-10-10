"""Check graders against original bugs, passing outputs, and targeted mutations."""
import json
from pathlib import Path
from cases import CASES, CHECK_PREAMBLE, ROUTER_CHECKS, CACHE_CHECKS, STREAM_CHECKS, run_checks

ROOT = Path(__file__).resolve().parent
SCRATCH = ROOT.parents[3] / "temp/prompt-ab-2026-10-10/grader-validation"
MUTATIONS = [
    '\n_base_resolve = resolve\n'
    'def resolve(*args, **kwargs):\n'
    '    result = _base_resolve(*args, **kwargs)\n'
    '    result["prompt"] = result["prompt"].strip()\n'
    '    return result\n',
    '\n_base_get = Cache.get\n'
    'async def _mutated_get(self, key, loader, ttl):\n'
    '    return await _base_get(self, key, loader, 999999 if ttl == 0 else ttl)\n'
    'Cache.get = _mutated_get\n',
    '\n_base_feed = Decoder.feed\n'
    'def _mutated_feed(self, chunk):\n'
    '    results = _base_feed(self, chunk)\n'
    '    for event in results:\n'
    '        event["data"] = event["data"].strip()\n'
    '    return results\n'
    'Decoder.feed = _mutated_feed\n',
]


def main():
    records = []
    for case, name, checks, mutation in zip(
        CASES[:3], ["router.py", "cache.py", "decoder.py"],
        [ROUTER_CHECKS, CACHE_CHECKS, STREAM_CHECKS], MUTATIONS
    ):
        passing = (ROOT / "runs" / f"v4-{case['id']}-default-r1/files" / name).read_text(encoding="utf-8")
        record = {"case": case["id"]}
        for variant, code in [("original", case["files"][name]),
                              ("positive", passing), ("mutant", passing + mutation)]:
            work = SCRATCH / case["id"] / variant
            work.mkdir(parents=True, exist_ok=True)
            (work / name).write_text(code, encoding="utf-8")
            record[variant] = run_checks(work, CHECK_PREAMBLE + checks)
        record["valid"] = (
            record["original"]["checks_passed"] < record["original"]["checks_total"]
            and record["positive"]["checks_passed"] == record["positive"]["checks_total"]
            and record["mutant"]["checks_passed"] < record["mutant"]["checks_total"]
        )
        records.append(record)
    (ROOT / "grader-validation.json").write_text(json.dumps(records, ensure_ascii=False, indent=2) + "\n",
                                                encoding="utf-8")
    print(json.dumps(records, ensure_ascii=False, indent=2))
    if not all(r["valid"] for r in records):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
