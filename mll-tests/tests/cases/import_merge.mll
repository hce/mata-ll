-- Repeated imports of ONE module merge (GHC semantics): each import
-- declaration contributes visibility — two Specific lists union, and a
-- qualified alias coexists with the unqualified forms.  Regression: a
-- seen-imports short-circuit dropped every import after a module's
-- first, so `import M (beta)` after `import M (alpha)` left beta
-- hidden, and a qualified alias after an unqualified import was never
-- introduced at all.  (That a lone Specific list still hides the rest
-- is pinned in compile_errors.rs.)

import MergeNames (alpha)
import MergeNames (beta)
import qualified MergeNames as M

main :: IO ()
main = do
    assert (alpha == 1) "first specific list"
    assert (beta == 2) "second specific list merged in"
    assert (M.gamma == 3) "qualified alias coexists"
    putStrLn "import merge ok"
