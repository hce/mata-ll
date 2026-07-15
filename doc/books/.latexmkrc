# Build the manuals with LuaLaTeX. They use fontspec with Latin Modern
# by name (\setmainfont{Latin Modern Roman}, \setmonofont{Latin Modern
# Mono}), which pdflatex cannot use at all and XeLaTeX cannot find
# unless the fonts are registered with the OS fontconfig. LuaLaTeX's
# loader finds them in the TeX tree, so it is the only engine that
# builds these out of the box. $pdf_mode = 4 makes `latexmk` (with no
# engine flag) use lualatex.
$pdf_mode = 4;
