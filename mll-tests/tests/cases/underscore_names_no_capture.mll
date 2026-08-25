-- User identifiers with a single leading underscore must never collide
-- with desugaring's fresh helper names.  Regression: sections desugared
-- with the binder `_sec` — a legal user identifier — so
-- `apply _sec = (_sec +) 5` became `\_sec -> _sec + _sec` (silent
-- capture: 10 + 5 computed 5 + 5).  Fresh names now live in the
-- reserved '__' namespace, which the lexer refuses in source (pinned
-- in compile_errors.rs).

apply :: Int -> Int
apply _sec = right + left
  where
    right = (_sec +) 5
    left = (+ _sec) 300

main :: IO ()
main = do
    print (apply 10)
    -- underscore names stay ordinary identifiers elsewhere
    let _tmp = 4
    print (_tmp * 2)
