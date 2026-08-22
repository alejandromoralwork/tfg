Ready for review
Select text to add comments on the plan
Correct order-lifecycle handling: cancellations
Context
The user was confused about where cancelled/partially-filled orders show up in the two L4 CSVs (order_statuses_accepted_PREVIEW.csv vs order_statuses_rejected_PREVIEW.csv) and worried the simulation might be misreading statusId/orderTypeId/tifId, causing the orderbook to not cancel orders the way the real venue did — i.e. silently diverging from real order-flow behavior.

Investigation (via Explore agent, cross-checking data/SCHEMA.md, data/sample/mapdir/{statuses,order_types,tifs}.csv, and empirical counts against both PREVIEW CSVs) found:

The accepted/rejected file split is not "live vs rejected." It's specifically "everything except the dominant badAloPxRejected ALO-price rejection stream" vs "badAloPxRejected only." Empirically, the accepted file's status column contains open(149), canceled(145), filled(2), and perpMarginRejected(4) — a rejection code, mixed right in with genuine lifecycle events. The rejected file is 100% badAloPxRejected. Cancelled orders live in the accepted file, not the rejected one. Zero oid overlap between the two files (consistent with the schema: an ALO-rejected oid never touched the book, so it can't also have lifecycle rows elsewhere).
This split doesn't actually matter to the current code, and doesn't need to — src/types.rs::Order::is_new_live_order() already gates purely on each row's own status_id/is_trigger, independent of which file it came from. That part is already correct.
The real gap: every non-open/triggered status_id — including all 8 cancellation-type codes (canceled, reduceOnlyCanceled, scheduledCancel, siblingFilledCanceled, selfTradeCanceled, marginCanceled, vaultWithdrawalCanceled, liquidatedCanceled) — is currently collapsed into one "not live, drop it" bucket by both FbaOrderBook::submit/CdaOrderBook::submit. There is no cancellation mechanism at all: a canceled row for an oid already sitting in pending_orders/bids/asks is just dropped instead of being used to remove that resting order. In a real replay, an order the trader genuinely cancelled just keeps sitting in our simulated book forever (until our own engine happens to match it) — a real fidelity gap, exactly what the user flagged.
tif_id is parsed and stored on every Order but never behaviorally read by either engine (no ALO-reject-if-crossing, no IOC-no-rest). Per user's decision below, this stays a documented limitation, not fixed now.
order_type_id is already handled correctly: Order::kind() collapses the 7 order_types.csv codes into Limit/Market (Stop Market/Take-Profit-Market → Market; Stop-Limit/Take-Profit-Limit → Limit), and conditional/trigger semantics are separately and correctly handled by is_trigger/triggered inside is_new_live_order(). No change needed here — confirmed correct, not a gap.
User decisions already confirmed:

Only cancellations are replayed, not fills. A cancellation is trader-initiated intent, independent of which matching engine processes the order flow, so it's fair (and necessary for fidelity) to replay. A filled status is Hyperliquid's own matching engine's outcome — irrelevant to what our independently-computed CDA/FBA engines decide, which is the entire point of comparing the two paradigms. Also practically simpler: a filled row's sz means "remaining after this fill," which would need reconciling against whatever our own engine already independently did to that same order — often ill-defined.
TIF stays unenforced for now — parsed/stored but not behaviorally read by either engine, same as today. Documented clearly as a known, intentional limitation (not a silent gap) rather than implemented this pass. (ALO in particular has no clean equivalent in FBA's discrete-batch model — no continuously-updating book to "cross" against at submission time — so it needs its own separate design discussion later if wanted.)
Implementation
1. src/types.rs — add Order::is_cancellation(&self) -> bool, alongside the existing is_new_live_order():

pub fn is_cancellation(&self) -> bool {
    matches!(self.status_id, 2 | 7 | 10 | 11 | 12 | 13 | 14 | 16)
}
(the 8 cancel-type codes from statuses.csv: canceled, reduceOnlyCanceled, scheduledCancel, siblingFilledCanceled, selfTradeCanceled, marginCanceled, vaultWithdrawalCanceled, liquidatedCanceled). Doc comment explains why filled(5) is deliberately excluded here (see decision #1 above) and points to the submit() doc comments in each engine for the full reasoning, so the "why" lives next to the code, not just in chat.

2. src/engines/fba.rs — add pub fn cancel(&mut self, oid: u64) -> bool (removes a matching-oid order from pending_orders via retain, returns whether anything was actually removed — a cancel for an unknown or already-cleared oid is a harmless no-op). Enhance submit():

pub fn submit(&mut self, order: Order) {
    if order.is_new_live_order() {
        self.total_submitted_qty = self.total_submitted_qty.saturating_add(order.remaining);
        self.pending_orders.push(order);
    } else if order.is_cancellation() {
        self.cancel(order.oid);
    }
    // else: a rejection or a `filled` event — deliberately ignored, see
    // `Order::is_cancellation` doc.
}
No change to total_submitted_qty on cancel — it correctly still counts as "was submitted," so fill_rate() correctly reflects that not everything submitted ends up filled.

3. src/engines/cda.rs — same shape: pub fn cancel(&mut self, oid: u64) -> bool (checks bids then asks via retain). submit() gains an early branch before its existing is_new_live_order() check:

pub fn submit(&mut self, mut order: Order) -> Vec<Trade> {
    if order.is_cancellation() {
        self.cancel(order.oid);
        return Vec::new();
    }
    if !order.is_new_live_order() || order.remaining == 0 {
        return Vec::new();
    }
    ... unchanged ...
}
4. No changes needed to src/inputs/simulator.rs or src/inputs/cli.rs. The loader already preserves each row's real status_id/oid (doesn't pre-filter), and the CLI's load handler already calls fba.submit(order) /cda.submit(order) uniformly for every parsed row — the new submit/cancel routing lives entirely inside each engine's own submit(), transparent to every existing caller. This is the same pattern already used for the is_new_live_order() gate.

5. src/inputs/test_suite.rs — new deterministic test cases (small helpers fn cancel_event(oid: u64, ts: u64) -> Order and fn filled_event(oid: u64, ts: u64) -> Order alongside the existing limit/market/non_live builders), added to both run_fba_tests() and run_cda_tests():

{fba,cda}_cancellation_removes_pending_order — submit a live order, then submit a canceled event for the same oid, assert it's gone from pending_orders / bids+asks.
{fba,cda}_cancellation_of_unknown_oid_is_harmless — submit a canceled event for an oid never seen live; assert no panic and the book is unaffected.
{fba,cda}_filled_status_does_not_touch_pending_order — locks in decision #1: submit a live order, then a filled-status event for the same oid, assert the order is still sitting there unchanged (proof the "fills aren't replayed" decision is actually implemented, not just documented).
Verification
cargo build / cargo test (toolchain confirmed working in this environment from prior sessions) — expect the existing 4 loader unit tests plus whichever new #[cfg(test)] tests get added to still pass.
test engine batch / test engine continuous from the running CLI — expect the existing 10 FBA + 11 CDA checklist entries to still all pass, plus the new cancellation-related entries (3 more per engine → 13 FBA / 14 CDA) to also pass.
Manual smoke test: load data/sample/order_statuses_accepted_PREVIEW.csv in FBA mode, then orderbook — the pending buffer should now correctly reflect that the 145 canceled rows in that sample removed their corresponding orders (for any oid that was also open earlier in the same load and hadn't cleared yet), rather than leaving them sitting in the buffer indefinitely.
Add Comment