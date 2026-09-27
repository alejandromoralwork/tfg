import re

defined = set()
for _f in ("Thesis/data/generated_numbers.tex", "Thesis/data/generated_verification.tex"):
    try:
        defined |= set(re.findall(r"newcommand\{\\(\w+)\}", open(_f, encoding="utf-8").read()))
    except FileNotFoundError:
        pass
print("defined macros:", len(defined))

for fname in ["Thesis/chapters/ch5_results.tex", "Thesis/chapters/ch4_simulation.tex", "Thesis/chapters/ch6_discussion.tex",
              "Thesis/chapters/appendix.tex"]:
    text = open(fname, encoding="utf-8").read()
    used = set(re.findall(r"\\([A-Za-z]+)", text))
    candidates = {m for m in used if re.match(r"^(Res|Desc|Sample|Val|Ver|Fba|Latency|Pricepaths|Price|Dispersion)", m)}
    missing = candidates - defined
    print(f"--- {fname} ---")
    print("candidate generated-style macros used:", len(candidates))
    print("MISSING:", sorted(missing))
