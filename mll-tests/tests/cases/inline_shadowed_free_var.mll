-- The call-site inliner must not relocate a body whose free names a
-- site LOCAL shadows: `addP x = x + p` (p a computed module constant,
-- so folding cannot erase the reference) inlined under `let p = 100`
-- read the local p — the wrong value, and a crash when the local's
-- type differed (this very shape: the let-bound p defaults to
-- Integer). The free-variable gate declines and the ordinary call
-- resolves the module constant.

module Main where
p :: Int
p = sum [1 .. 4]
addP :: Int -> Int
addP x = x + p
main :: IO ()
main = do
    let p = 100
    print (addP 1)
    print p
