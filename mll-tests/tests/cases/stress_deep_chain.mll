-- Regression: long, non-foldable operator/cons chains must compile to Lua
-- that Lua can actually LOAD. Before the expression-splitting pass
-- (mllc/src/split.rs), a whole body was emitted as one deeply-nested Lua
-- expression; past Lua's parser nesting limit (~200 levels) the generated
-- .lua failed to load with "C stack overflow" (LuaJIT: "chunk has too many
-- syntax levels") before the program ever ran. Because a `<>`/`:` operand
-- lowers to ~2-3 nested Lua levels each, these chains overflowed at ~65-70
-- operands unsplit; at 150 they are well past that. The pass pulls deep
-- sub-expressions into `let` bindings (flat sibling locals) so nesting
-- stays bounded. Operands are top-level CAFs so folding cannot collapse them.

a :: Int
a = 1
b :: Int
b = 2

chainSum :: Int
chainSum = a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b+a+b

sx :: String
sx = "x"
sy :: String
sy = "y"

chainStr :: String
chainStr = sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy <> sx <> sy

x :: Int
x = 7

chainList :: [Int]
chainList = x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : x : []

main :: IO ()
main = do
    assert (chainSum == 225) "deep arithmetic chain"
    assert (chainStr == "xyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxyxy") "deep <> concat chain"
    assert (sum chainList == 1050) "deep cons chain"
    putStrLn "deep chains: OK"
