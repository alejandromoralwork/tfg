import os
os.chdir(r"C:\Users\pc\other\Thesis")
s = open("preamble.tex", encoding="utf8").read()
a = "\usepackage[a4paper,left=3.0cm,right=2.5cm,top=2.5cm,bottom=2.5cm]{geometry}\n"
assert s.count(a) == 1
s = s.replace(a, "% page geometry is set once, in main.tex (2 cm margins)\n")
b = "\theoremstyle{remark}\n\newtheorem{hypothesisx}{Prediction}[chapter]"
assert s.count(b) == 1
s = s.replace(b, "\newtheorem{lemma}{Lemma}[chapter]\n\theoremstyle{remark}\n\newtheorem{remark}{Remark}[chapter]\n\newtheorem{hypothesisx}{Prediction}[chapter]")
open("preamble.tex", "w", encoding="utf8").write(s)
f = open("frontmatter.tex", encoding="utf8").read()
c = "\ \Supervisor Hartwig Mayer"
assert f.count(c) == 1
open("frontmatter.tex", "w", encoding="utf8").write(f.replace(c, "\ Hartwig Mayer"))
