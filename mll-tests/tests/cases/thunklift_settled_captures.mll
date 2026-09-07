-- Closure-free thunk lifting captures VALUES at allocation; a multi-binding
-- let is emitted as forward-declared locals assigned in order, whose
-- suspensions used to keep the closure form (the captured name is
-- assigned after its declaration). A binding assigned exactly once, before
-- the suspension that captures it, now lifts — and the program must behave
-- exactly as before: later bindings see the earlier ones' final values,
-- an undemanded binding (a bottom included) is never evaluated, and a
-- suspension with four captures lifts too.

data P = P Int Int deriving Show

build :: Int -> Int -> Int -> Int -> P
build a b c d =
    let s1 = a + b
        s2 = s1 * c
        bomb = error "never demanded" :: Int
        four = if s1 > 0 then a * 1000 + b * 100 + c * 10 + d else bomb
        pick = if d > 0 then four else s2
    in P pick (s2 + 1)

keepLazy :: Int -> Int
keepLazy n =
    let deep = error "still never demanded" :: Int
        chosen = if n > 0 then n else deep
    in chosen

main :: IO ()
main = do
    print (build 1 2 3 4)
    print (build 1 2 3 0)
    print (keepLazy 5)
    print (map (\k -> keepLazy k) [1, 2, 3])
