"""Aggregate observed results without calling a judge model."""
import hashlib
import json
from pathlib import Path
import statistics

ROOT = Path(__file__).resolve().parent
ARMS = ["default", "draft", "draft_keep_runtime"]
CASES = ["routing", "async_cache", "stream_decoder", "explore", "review_injection", "blocked_validation"]


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def normalize(value, trial_id):
    if isinstance(value, str):
        return value.replace(trial_id, "<TRIAL>").replace("\\", "/")
    if isinstance(value, list):
        return [normalize(x, trial_id) for x in value]
    if isinstance(value, dict):
        return {k: normalize(v, trial_id) for k, v in value.items() if k != "id"}
    return value


def main():
    manual_path = ROOT / "manual-review.json"
    manual = load(manual_path) if manual_path.exists() else {}
    results = []
    requests = []
    user_contexts = {}
    runtime_contexts = {}
    for case in CASES:
        for repeat in (1, 2):
            for arm in ARMS:
                trial_id = f"v4-{case}-{arm}-r{repeat}"
                directory = ROOT / "runs" / trial_id
                if not (directory / "result.json").exists():
                    continue
                result = load(directory / "result.json")
                if trial_id in manual:
                    result["manual_review"] = manual[trial_id]
                grade_passed = manual.get(trial_id, {}).get("passed", result["grading"]["passed"])
                result["success"] = bool(result["exit_code"] == 0 and result["turn_completed"]
                                         and not result["timed_out"] and grade_passed)
                stderr = (directory / "stderr.txt").read_text(encoding="utf-8")
                result["command_policy_rejections"] = sum(
                    "exec_command failed" in line and "blocked by policy" in line
                    for line in stderr.splitlines())
                events = [json.loads(line) for line in (directory / "events.jsonl").read_text(encoding="utf-8").splitlines()]
                result["parent_repo_instructions_read"] = any(
                    "# CodexElves 项目工作规则" in e.get("item", {}).get("aggregated_output", "") for e in events)
                records = load(directory / "requests.json")
                result["initial_input_tokens"] = records[0].get("usage", {}).get("input_tokens")
                results.append(result)
                requests.extend(records)
                user_contexts[trial_id] = normalize(load(directory / "initial_user_messages.json"), trial_id)
                runtime_contexts[trial_id] = normalize(load(directory / "initial_runtime_messages.json"), trial_id)
    summaries = {}
    for arm in ARMS:
        rows = [r for r in results if r["arm"] == arm]
        if not rows:
            continue
        def total(field):
            return sum((r.get("usage") or {}).get(field, 0) for r in rows)
        summaries[arm] = {
            "runs": len(rows), "successes": sum(r["success"] for r in rows),
            "coding_successes": sum(r["success"] for r in rows if r["case"] in CASES[:3]),
            "coding_runs": sum(r["case"] in CASES[:3] for r in rows),
            "coding_input_tokens": sum((r.get("usage") or {}).get("input_tokens", 0) for r in rows if r["case"] in CASES[:3]),
            "coding_output_tokens": sum((r.get("usage") or {}).get("output_tokens", 0) for r in rows if r["case"] in CASES[:3]),
            "coding_seconds_sum": round(sum(r["seconds"] for r in rows if r["case"] in CASES[:3]), 3),
            "input_tokens": total("input_tokens"),
            "cached_input_tokens": total("cached_input_tokens"),
            "uncached_input_tokens": total("input_tokens") - total("cached_input_tokens"),
            "output_tokens": total("output_tokens"),
            "reasoning_output_tokens_reported": total("reasoning_output_tokens"),
            "tool_calls": sum(r["tool_count"] for r in rows),
            "model_requests": sum(r["request_count"] for r in rows),
            "command_policy_rejections": sum(r["command_policy_rejections"] for r in rows),
            "runs_reading_parent_instructions": sum(r["parent_repo_instructions_read"] for r in rows),
            "seconds_sum": round(sum(r["seconds"] for r in rows), 3),
            "seconds_median": round(statistics.median(r["seconds"] for r in rows), 3),
            "final_chars_median": statistics.median(r["final_chars"] for r in rows),
        }
    pair_checks = []
    for case in CASES:
        for repeat in (1, 2):
            ids = [f"v4-{case}-{arm}-r{repeat}" for arm in ARMS]
            if not all(key in user_contexts for key in ids):
                continue
            pair_checks.append({
                "case": case, "repeat": repeat,
                "identical_initial_user_context_after_path_and_id_normalization":
                    all(user_contexts[key] == user_contexts[ids[0]] for key in ids),
                "identical_runtime_before_override_after_path_and_id_normalization":
                    all(runtime_contexts[key] == runtime_contexts[ids[0]] for key in ids),
            })
    aggregate = {
        "formal_trials_completed": len(results), "expected_trials": 36,
        "summaries": summaries,
        "by_case": {
            case: {
                arm: {
                    "runs": len([r for r in results if r["case"] == case and r["arm"] == arm]),
                    "successes": sum(r["success"] for r in results if r["case"] == case and r["arm"] == arm),
                    "input_tokens": sum((r.get("usage") or {}).get("input_tokens", 0) for r in results if r["case"] == case and r["arm"] == arm),
                    "output_tokens": sum((r.get("usage") or {}).get("output_tokens", 0) for r in results if r["case"] == case and r["arm"] == arm),
                    "seconds_sum": round(sum(r["seconds"] for r in results if r["case"] == case and r["arm"] == arm), 3),
                } for arm in ARMS
            } for case in CASES
        },
        "integrity": {
            "request_count": len(requests),
            "requested_models": sorted({r["model"] for r in requests}),
            "response_models": sorted({r.get("response_model", "") for r in requests}),
            "reasoning_settings": sorted({json.dumps(r["reasoning"], sort_keys=True) for r in requests}),
            "distinct_tool_schema_hashes": sorted({r["tools_sha256"] for r in requests}),
            "instruction_hashes": sorted({r["instructions_sha256"] for r in requests}),
            "non_200_requests": sum(r.get("http_status") != 200 for r in requests),
            "transport_errors": sum(bool(r.get("transport_error")) for r in requests),
            "input_equivalence": pair_checks,
            "cases_sha256": hashlib.sha256((ROOT / "cases.py").read_bytes()).hexdigest(),
        },
        "results": results,
    }
    (ROOT / "results.json").write_text(json.dumps(aggregate, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in aggregate.items() if k not in ("results", "integrity", "by_case")},
                     ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
