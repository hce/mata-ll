-- A re-export CHAIN: this file imports only ReexOuter, which re-exports
-- ReexUmbrella wholesale, which in turn re-exports ReexInner and names
-- outerFn — so both reach here through two hops. ReexUmbrella's private
-- outerPrivate must NOT leak (it is renamed away one level down); the
-- rejection side of that is covered by compile_errors.rs.

import ReexOuter

main :: IO ()
main = do
    assert (innerFn 4 == 5) "chained re-export, two hops"
    assert (outerFn 4 == 10) "chained re-export, one hop"
