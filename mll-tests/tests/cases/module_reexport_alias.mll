-- Re-export through a qualified ALIAS: ReexAlias exports `module RI`,
-- where RI aliases its (also unqualified) import of ReexInner. This file
-- imports only ReexAlias, so ReexInner's names reach here through that
-- alias entry alone.

import ReexAlias

main :: IO ()
main = do
    assert (innerFn 4 == 5) "alias re-export, function"
    assert (innerTag == "inner") "alias re-export, value"
