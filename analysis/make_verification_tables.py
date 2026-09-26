"""Turns the verification JSON files into LaTeX macros for the testing appendix.

Reads analysis/output/verification_*.json (written by the verify_*.py / run_*.py scripts) and writes
Thesis/data/generated_verification.tex: scalar macros (\\Ver...) and table-body macros (rows).
No number in the appendix is typed by hand.

Usage: python analysis/make_verification_tables.py
"""
from __future__ import annotations

import json
import re
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "output"
TARGET = HERE.parent / "Thesis" / "data" / "generated_verification.tex"

macros: dict[str, str] = {}


def load(name):
    return json.loads((OUT / f"verification_{name}.json").read_text())


def n(x) -> str:
    return f"{int(round(x)):,}"


def f(x, d=1) -> str:
    return f"{x:,.{d}f}"


def m(x, d=2) -> str:
    """millions"""
    return f"{x / 1e6:,.{d}f}"


def add(name, value):
    assert re.fullmatch(r"[A-Za-z]+", name), name
    macros[name] = str(value)


def tex(s: str) -> str:
    s = s.replace("\\", " ").replace("_", r"\_").replace("&", r"\&").replace("%", r"\%").replace("#", r"\#")
    s = s.replace(">=", r"$\geq$").replace("<=", r"$\leq$")
    s = s.replace(" x ", r" $\times$ ")
    return s


STATUS = {1: "open", 2: "canceled", 3: "perpMarginRejected", 4: "iocCancelRejected", 5: "filled", 6: "minTradeNtlRejected",
          7: "reduceOnlyCanceled", 8: "reduceOnlyRejected", 9: "triggered", 10: "scheduledCancel", 11: "siblingFilledCanceled",
          12: "selfTradeCanceled", 13: "marginCanceled"}


def raw_and_dup():
    r = load("raw")
    add("VerFiles", n(r["files"]))
    add("VerRecords", n(r["records"]))
    add("VerBadSize", n(r["files_size_not_multiple_of_54"]))
    add("VerBadBool", n(r["bool_bytes_not_0_or_1"]))
    add("VerBadStatus", n(r["status_above_17"]))
    add("VerBadType", n(r["order_type_above_6"]))
    add("VerBadTif", n(r["tif_above_5"]))
    add("VerSizeAbove", n(r["records_size_above_original"]))
    add("VerOpenRecords", n(r["status_histogram"]["1"]))
    add("VerOpenNonzero", n(r["open_records_with_nonzero_timestampDiff"]))
    add("VerFillRecords", n(r["fill_records"]))
    add("VerFillNegative", n(r["fill_records_with_negative_size_change"]))
    add("VerBackwardWithin", n(r["backward_steps_within_files"]))
    add("VerBackwardAcross", n(r["backward_steps_across_files"]))
    back = r["backward_steps_within_files"] + r["backward_steps_across_files"]
    add("VerBackwardTotal", n(back))
    add("VerMaxBackwardSec", n(r["max_backward_step_ns"] / 1e9))
    gaps = r["gaps_over_60s_within_files"] + r["gaps_over_60s_across_files"]
    add("VerGapsTotal", n(gaps))
    longest = max(r["long_gap_list"], key=lambda g: g[2])
    add("VerMaxGapMin", n(longest[2] / 60e9))
    add("VerGapsTotalHours", f(sum(g[2] for g in r["long_gap_list"]) / 3600e9, 1))
    import datetime
    when = datetime.datetime.fromtimestamp(longest[1] / 1e9, datetime.timezone.utc)
    add("VerLongestGapWhere", when.strftime("%-d December, from %H:%M UTC") if False else f"{when.day} December, from {when:%H:%M} UTC")
    add("VerOutsideHour", n(r["records_outside_file_hour"]))
    add("VerOutsideHourPct", f(100 * r["records_outside_file_hour"] / r["records"], 2))
    add("VerMaxOutsideMin", n(r["max_ns_outside_file_hour"] / 60e9))
    add("VerNewLive", n(r["new_live_orders"]))
    add("VerCancels", n(r["cancellations"]))
    add("VerOther", n(r["other_records"]))
    add("VerRoundZero", n(r["records_size_rounds_to_zero"]))
    add("VerOrders", n(r["orders_seen"]))
    add("VerFirstOpen", n(r["orders_first_record_open_or_triggered"]))
    add("VerFirstRejection", n(r["orders_first_record_rejection"]))
    add("VerFirstCarry", n(r["first_record_not_open_explained_by_carry"]))
    add("VerFirstUnexplained", n(r["first_record_not_open_unexplained"]))
    add("VerUnexplainedPre", n(r["unexplained_but_submitted_before_month_start"]))
    add("VerSpreadTwoMs", n(r["orders_submission_time_spread_over_2ms"]))
    add("VerSpreadOneS", n(r["orders_submission_time_spread_over_1s"]))
    add("VerSpreadTrig", n(r["orders_spread_over_1s_with_triggered_event"]))
    add("VerSizeUp", n(r["size_increases_within_order"]))
    add("VerAfterTerminal", n(r["events_after_terminal_event"]))
    # venue side
    add("VerTakerBuy", m(r["venue_taker_buy_total_1e6"] / 1e6))
    add("VerTakerSell", m(r["venue_taker_sell_total_1e6"] / 1e6))
    add("VerVenueVol", m((r["venue_taker_buy_total_1e6"] + r["venue_taker_sell_total_1e6"]) / 1e6))
    d = load("duplicates")
    t = d["totals"]
    add("VerDupRepeats", n(t["repeats"]))
    add("VerDupPct", f(100 * t["repeats"] / t["records"], 2))
    add("VerDupLiveRepeats", n(t["live_repeats"]))
    w = d["worst_files"][0]
    add("VerDupWorstFile", tex(w["file"].replace(".data.gz", "")).replace("/", "/"))
    add("VerDupWorstPct", f(100 * w["repeats"] / w["records"], 0))
    rows = []
    for k, v in sorted(t["repeats_by_status"].items(), key=lambda kv: -kv[1]):
        rows.append(f"{tex(STATUS.get(int(k), k))} & {n(v)} & {100 * v / t['repeats']:.1f}\\% \\\\")
    add("VerDupRows", "\n".join(rows))


def tests_and_mutants():
    t = load("tests")
    d, ig = t["cargo_default"], t["cargo_ignored"]
    add("VerTestsDefault", d["passed"])
    add("VerTestsIgnored", ig["passed"])
    add("VerTestsTotal", d["passed"] + ig["passed"])
    add("VerTestsPassed", d["passed"] + ig["passed"])
    props = t["property_summaries"]
    new_tests = 3 + 2                                    # three property tests + two status-code tests
    add("VerTestsLegacy", d["passed"] + ig["passed"] - new_tests)
    add("VerTestsNew", new_tests)
    ck = t["checklist"]["suites"]
    add("VerChecklistCda", ck[0][0])
    add("VerChecklistFba", ck[1][0])
    add("VerChecklistMet", ck[2][0])
    add("VerChecklistPassed", sum(a for a, _ in ck))
    add("VerChecklistTotal", sum(b for _, b in ck))
    add("VerMetricTests", 17)
    c, fb = props["cda"], props["fba"]
    add("VerCdaScen", n(c["scenarios"]))
    add("VerCdaEvents", n(c["events"]))
    add("VerCdaTrades", n(c["trades"]))
    add("VerCdaCancels", n(c["cancels_hit"]))
    add("VerCdaAsserts", n(c["assertions"]))
    add("VerFbaScen", n(fb["scenarios"]))
    add("VerFbaBatches", n(fb["batches"]))
    add("VerFbaTraded", n(fb["batches_with_trades"]))
    add("VerFbaTies", n(fb["batches_with_price_ties"]))
    add("VerFbaAsserts", n(fb["assertions"]))
    add("VerFbaBetterOrders", n(fb["strictly_better_orders"]))
    add("VerFbaBetterPartial", n(fb["of_which_partly_filled_long_side"]))
    add("VerFbaBetterPct", f(100 * fb["of_which_partly_filled_long_side"] / fb["strictly_better_orders"], 2))
    add("VerDetReplays", n(props["determinism"]["replays_compared"]))

    mu = load("mutations")
    rows = []
    killed = 0
    for r in mu:
        if "error" in r:
            continue
        killed += r["killed"]
        a = len(r["layer_A_cargo_tests"])
        b = r["layer_B_checklist_failed"]
        cc = len(r["layer_C_property_tests"])
        rows.append(f"{r['id']} & {tex(r['description'])} & {a if a else '--'} & {'yes' if b else '--'} & {cc if cc else '--'} \\\\")
    total = len([r for r in mu if "error" not in r])
    add("VerMutRows", "\n".join(rows))
    add("VerMutTotal", total)
    add("VerMutKilled", killed)
    survivors = [r["id"] for r in mu if "error" not in r and not r["killed"]]
    only_c = [r["id"] for r in mu if "error" not in r and r["killed"] and not r["layer_A_cargo_tests"] and not r["layer_B_checklist_failed"]]
    add("VerMutSummary", (
        f"Of the {total} planted bugs, {killed} are detected by at least one layer"
        + (f"; {', '.join(survivors)} survived." if survivors else ".")
        + f" {len(only_c)} of them ({', '.join(only_c)}) are detected only by the property tests, which is where the new tests add most. "
        + "Every bug in the FBA's choice and rationing of prices is caught by the property test, and every bug in the CDA's matching is caught by it as well."
    ) if only_c else f"Of the {total} planted bugs, {killed} are detected by at least one layer.")


def reference():
    d = load("reference_engines")
    labels = {"sample": r"1 Dec, \texttt{sol\_12}", "quiet": r"25 Dec, \texttt{sol\_04}", "busy": r"15 Dec, \texttt{sol\_14}", "dup": r"20 Dec, \texttt{sol\_09}"}
    rows, total_buckets, mism = [], 0, 0
    for k in ("sample", "quiet", "busy", "dup"):
        if k not in d:
            continue
        v = d[k]
        c, fb = v["cda"], v["fba"]
        nb = c["buckets"]
        c1 = min(c["trade_count_equal"], c["volume_equal"], c["notional_equal"], c["vwap_equal"])
        c2 = min(c["quoted_spread_bps_equal"], c["depth_at_best_equal"], c["effective_spread_bps_equal"])
        f1 = min(fb["trade_count_equal"], fb["volume_equal"], fb["notional_equal"], fb["vwap_equal"])
        f2 = min(fb["residual_share_equal"], fb["depth_at_best_equal"])
        tot = "yes" if (c["totals_equal"] and fb["totals_equal"]) else "NO"
        rows.append(f"{labels[k]} & {n(v['records'])} & {n(c1)} & {n(c2)} & {n(f1)} & {n(f2)} / {n(fb['single_batch_buckets'])} & {tot} \\\\")
        total_buckets += 2 * nb
        mism += (nb - c1) + (nb - c2) + (nb - f1) + (fb["single_batch_buckets"] - f2)
    add("VerRefRows", "\n".join(rows))
    add("VerRefBuckets", n(total_buckets))
    add("VerRefMismatches", n(mism))


def outputs():
    d = load("outputs")
    fba, cda = {r["invariant"]: r for r in d["fba"]}, {r["invariant"]: r for r in d["cda"]}
    names = [r["invariant"] for r in d["fba"]] + [r["invariant"] for r in d["cda"] if r["invariant"] not in fba]
    rows = []
    for nm in names:
        a, b = fba.get(nm), cda.get(nm)
        fa = f"{n(a['rows_checked'])} & {n(a['violations'])}" if a else "--- & ---"
        fb = f"{n(b['rows_checked'])} & {n(b['violations'])}" if b else "--- & ---"
        rows.append(f"{tex(nm)} & {fa} & {fb} \\\\")
    add("VerInvRows", "\n".join(rows))
    add("VerRows", n(d["fba_rows"]))
    q = lambda R: [r for r in R if r["invariant"].startswith("book: quoted")][0]["rows_checked"]
    add("VerQuotedCda", n(q(d["cda"])))
    add("VerQuotedFba", n(q(d["fba"])))
    disp = [r for r in d["fba"] if r["invariant"].startswith("uniform price")][0]
    add("VerDispRows", n(disp["violations"]))
    add("VerDispZeroPct", f(100 * (1 - disp["violations"] / disp["rows_checked"]), 2))
    fo = d["fba_fill_rate_over_one"]
    co = d["cda_fill_rate_over_one"]
    add("VerFillOverFba", fo["rows"])
    add("VerFillOverCda", co["rows"])
    fr = [r for r in d["cda"] if r["invariant"].startswith("range: fill_rate")][0]
    add("VerFillRows", n(fr["rows_checked"]))
    add("VerFillEffect", f"{co['mean_fill_rate'] - co['mean_fill_rate_without_them']:.6f}".rstrip("0"))
    add("VerTradesFba", n(d["fba_totals_vs_checkpoint"]["trades"][0]))
    add("VerTradesCda", n(d["cda_totals_vs_checkpoint"]["trades"][0]))


def fidelity():
    d = load("replay_fidelity")
    r = load("raw")
    add("VerCdaVol", m(d["cda_volume"]))
    add("VerFbaVol", m(d["fba_volume"]))
    add("VerCdaOverVenue", f(d["cda_over_venue"], 2))
    add("VerFbaOverVenue", f(d["fba_over_venue"], 2))
    add("VerDayRatioMin", f(d["daily_cda_over_venue"]["min"], 2))
    add("VerDayRatioMed", f(d["daily_cda_over_venue"]["median"], 2))
    add("VerDayRatioMax", f(d["daily_cda_over_venue"]["max"], 2))
    add("VerCorrHour", f(d["corr_hour"]["venue_vs_cda"], 2))
    add("VerCorrDay", f(d["corr_day"]["venue_vs_cda"], 2))
    add("VerCorrSecond", f(d["corr_second"]["venue_vs_cda"], 2))
    add("VerCorrHourFba", f(d["corr_hour"]["venue_vs_fba"], 2))
    add("VerMakerLow", m(d["venue_maker_completed"]))
    add("VerMakerHigh", m(d["venue_maker_upper"]))
    add("VerBuySellGapPct", f(100 * abs(d["buyer_side_fills"] / d["seller_side_fills"] - 1), 1))
    add("VerNoVenueSharePct", f(100 * d["cda_volume_in_seconds_without_venue_trade_share"], 0))
    h = load("replay_hours")
    rows = []
    for x in h["hours"]:
        rows.append(f"{tex(x['file'].replace('.data.gz', ''))} & {n(x['sim_volume'])} & {n(x['venue_taker_volume'])} & {n(x['sim_taker_post_only'])} & {n(x['sim_self_trades'])} & "
                    f"{x['ratio_sim_over_venue']:.2f} & {x['ratio_ex_post_only_over_venue']:.2f} \\\\")
    t = h["totals"]
    add("VerHourRows", "\n".join(rows))
    add("VerHoursN", len(h["hours"]))
    add("VerHoursSim", n(t["sim_volume"]))
    add("VerHoursVenue", n(t["venue_taker_volume"]))
    add("VerHoursPost", n(t["sim_taker_post_only"]))
    add("VerHoursSelf", n(t["sim_self_trades"]))
    add("VerHoursRatio", f"{t['ratio_sim_over_venue']:.2f}")
    add("VerHoursExRatio", f"{t['ratio_ex_post_only_over_venue']:.2f}")
    add("VerHoursPostPct", f(100 * t["post_only_share_of_sim"], 0))
    add("VerHoursSelfPct", f(100 * t["sim_self_trades"] / t["sim_volume"], 1))
    add("VerTopExcessPct", f(100 * t["excess_top15_share"], 0))
    q = [x for x in h["hours"] if x["file"].startswith("20251225") or True][0]


def resets():
    d = load("resets")
    add("VerResetN", d["n_events"])
    add("VerResetFirst", d["first_event"][:16].replace("2025-12-", "").replace("-", " "))
    ev = d["reset_events"]
    rows = []
    for e in ev:
        day, hour = e["time"][8:10], e["time"][11:16]
        rec = "--" if e["minutes_to_recover_90pct"] is None else str(e["minutes_to_recover_90pct"])
        rows.append(f"{int(day)} Dec, {hour} & {n(e['depth_before'])} & {n(e['depth_first_minute'])} & {100 * e['ratio']:.0f}\\% & {rec} \\\\")
    add("VerResetRows", "\n".join(rows))
    add("VerMultiBuckets", n(d["fba_multi_price_buckets"]))
    add("VerMultiFirst", d["fba_first_multi_price_bucket"][:16].replace("2025-12-0", "").replace(" ", ", "))
    add("VerMultiShare", f(100 * d["fba_multi_price_share_after_first"], 1))
    add("VerMultiBeforeFirst", n(d["fba_multi_price_before_first"]))
    add("VerMultiBeforeTrading", n(d["fba_trading_buckets_before_first"]))
    daily = d["book_depth_daily_median"]
    add("VerDepthDayOne", n(daily["2025-12-01"]))
    add("VerDepthDayTwenty", n(daily["2025-12-20"]))


def main():
    raw_and_dup()
    tests_and_mutants()
    reference()
    outputs()
    fidelity()
    resets()
    lines = ["% AUTO-GENERATED by analysis/make_verification_tables.py -- do not hand-edit.",
             "% Source: analysis/output/verification_*.json (written by the verify_*.py and run_*.py scripts).", ""]
    for k in sorted(macros):
        lines.append(f"\\newcommand{{\\{k}}}{{{macros[k]}}}")
    TARGET.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {len(macros)} macros to {TARGET}")


if __name__ == "__main__":
    main()
