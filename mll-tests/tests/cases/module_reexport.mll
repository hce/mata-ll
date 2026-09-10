-- Module re-exports (Haskell 2010 §5.2): the umbrella idiom
-- (ReexUmbrella passes `module ReexInner` on) and the self re-export
-- (ReexSelf exports everything via `module ReexSelf`). The negative
-- sides — an export list gating off transitive names, re-exporting a
-- module that is not imported — live in compile_errors.rs.

import ReexUmbrella
import ReexSelf

main :: IO ()
main = do
    -- innerFn/innerTag/InnerT come from ReexInner, visible only through
    -- ReexUmbrella's `module ReexInner` entry.
    assert (innerFn 4 == 5) "re-exported function"
    assert (innerTag == "inner") "re-exported value"
    assert (MkInnerT 3 == MkInnerT 3) "re-exported type and constructor"
    -- outerFn is named individually in the same export list.
    assert (outerFn 4 == 10) "own name next to a module re-export"
    -- The self re-export covers every own declaration.
    assert (selfA 1 == 11) "self re-export, first"
    assert (selfB 1 == 12) "self re-export, second"
