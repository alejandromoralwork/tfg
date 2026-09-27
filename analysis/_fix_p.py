def sub(p, pairs):
    s = open(p, encoding="utf8").read()
    for a, b in pairs:
        assert s.count(a) == 1, (p, a[:50], s.count(a))
        s = s.replace(a, b)
    open(p, "w", encoding="utf8").write(s)

sub("ch5_results.tex", [
("submitted limit price that uniform pricing gives. Prediction~P9 says it should\nbe positive for every FBA order that is not marginal; the useful comparison is",
 "submitted limit price that uniform pricing gives. Theory says it should\nbe positive for every FBA order that is not marginal; the useful comparison is"),
("trader surplus is slightly higher, in line with prediction~P9; and order size",
 "trader surplus is slightly higher, as uniform pricing implies; and order size"),
("Predictions P5, P6 and P7 remain out of reach for the reason given in\n\Cref{sec:testable}: every result above is the mechanical effect of one fixed\nrule on orders that were not submitted in reaction to it. P1 holds by\nconstruction for most intervals, and P8 and the rationing comparison were not\nrun, so this chapter does not claim to have tested them.",
 "The behavioural predictions (quotes narrowing as market makers requote, order\nsizes inflating under pro rata, orders bunching before the batch boundary, bids\nshaded under price uncertainty) remain out of reach for the reason given in\n\Cref{sec:testable}: every result above is the mechanical effect of one fixed\nrule on orders that were not submitted in reaction to it. Zero price dispersion\nwithin a batch holds by construction for most intervals, and the effect of\n$\tau$ on batch composition and the rationing comparison were not run, so this\nchapter does not claim to have tested them."),
])
sub("ch6_discussion.tex", [
("use at scale & Not testable here (P7)\\\\", "use at scale & Not testable here (behavioural)\\\\"),
("The nine predictions carried through this thesis split by testability, and\nthe split bounds every claim made.",
 "The predictions carried through this thesis split by testability, and\nthe split bounds every claim made."),
("\textbf{Mechanical predictions are established.} P1 (zero intra-interval\ndispersion), P4 (the delay lower bound), P8 (the effect of $\tau$ on batch\ncomposition), and P9 (positive surplus relative to submitted limits) follow",
 "\textbf{Mechanical predictions are established.} Zero intra-interval\ndispersion, the delay lower bound, the effect of $\tau$ on batch\ncomposition, and positive surplus relative to submitted limits follow"),
("\textbf{One prediction is partly reachable.} P2, that batching reduces the",
 "\textbf{One prediction is partly reachable.} That batching reduces the"),
("\textbf{Behavioural predictions are out of reach.} P3, P5, P6 and P7 all\nrequire",
 "\textbf{Behavioural predictions are out of reach.} The remaining ones all\nrequire"),
])
sub("ch7_conclusion.tex", [
("making the equilibrium predictions P3, P5, P6 and P7 testable", "making the behavioural (equilibrium) predictions testable"),
])
