-- Regression test: a name hidden transitively by one import must still be
-- usable when imported explicitly by another (an explicit import overrides
-- transitive selection-hiding). DiamondMid is imported first; it transitively
-- merges DiamondLeaf's leafB and hides it (leafB is not in DiamondMid's
-- wanted set). The later, explicit `import DiamondLeaf (leafB)` must win.
-- Before the fix, hidden_names was monotonic and leafB stayed hidden,
-- failing with "leafB is not exported by its module".

import DiamondMid (midFn)
import DiamondLeaf (leafB)

main :: IO ()
main = do
    assert (midFn 10 == 12) "midFn from the middle module"
    assert (leafB 10 == 20) "leafB imported explicitly despite transitive hiding"
    putStrLn "diamond_import ok"
