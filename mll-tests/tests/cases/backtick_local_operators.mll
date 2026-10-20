-- A local binder (parameter, let or where binding) named `div`, `mod`,
-- `quot`, `rem` or `seq` is what every pass sees for that name, in
-- backtick and in prefix form: the constant folder, the demand analyses
-- (a local that ignores an operand carrying `error` keeps it unevaluated),
-- the call-site and fused-pipeline inliners (an inlined body's builtin
-- stays the builtin under a call-site local of the same spelling) and
-- the emitter. Import resolution respells such a binder, so the spelling
-- denotes the builtin everywhere else (modules.rs, `Rename::local`).
-- 2026-10 fresh-eyes review (O3, N2, N3): closed by the module resolver
-- (W2/W10); `seq` joined the claimed spellings here; pinned here.
import Prelude hiding (rem)

rem :: Int -> Int -> Int
rem a b = a * 10 + b

whereDiv :: Int -> Int -> Int
whereDiv x y = (x `div` y) + div y x
  where div a b = a * 100 + b

whereMod :: Int -> Int -> Int
whereMod x y = (x `mod` y) * mod x y
  where mod a b = a + b

whereQuot :: Int -> Int -> Int
whereQuot x y = x `quot` y
  where quot _ b = b

whereRem :: Int -> Int -> Int
whereRem x y = (x `rem` y) + rem x y
  where rem a _ = a

whereSeq :: Int -> Int -> Int
whereSeq x y = x `seq` y
  where seq _ b = b * 2

letDiv :: Int -> Int
letDiv k = let div a b = a + b + k in 7 `div` 2

letMod :: Int -> Int
letMod k = let mod a b = a * b + k in mod 3 4 + (3 `mod` 4)

letQuot :: Int -> Int
letQuot k = let quot a _ = a + k in 7 `quot` (error "lazy1")

letRem :: Int -> Int
letRem k = let rem _ b = b + k in rem 1 2 + (1 `rem` 2)

letSeq :: Int -> Int
letSeq k = let seq a _ = a + k in 7 `seq` (error "lazy2")

applyDiv :: (Int -> Int -> Int) -> Int
applyDiv div = 10 `div` 2

applyMod :: (Int -> Int -> Int) -> Int
applyMod mod = mod 10 3 + (10 `mod` 3)

applyQuot :: (Int -> Int -> Int) -> Int
applyQuot quot = 9 `quot` 2

applyRem :: (Int -> Int -> Int) -> Int
applyRem rem = 9 `rem` 2

applySeq :: (Int -> Int -> Int) -> Int
applySeq seq = 1 `seq` 2

half :: Int -> Int
half x = x `div` 2

underDiv :: (Int -> Int -> Int) -> Int -> Int
underDiv div n = half n + div n 1

third :: Int -> Int
third x = x `mod` 3

underMod :: (Int -> Int -> Int) -> [Int] -> Int
underMod mod xs = sum (map third xs) + mod 1 1

seventh :: Int -> Int
seventh x = x `quot` 7

underQuot :: (Int -> Int -> Int) -> [Int] -> Int
underQuot quot xs = sum (map seventh xs) + quot 1 2 + helper 10
  where helper y = y `quot` 3

underLet :: Int -> Int
underLet n = inner n + helper 3
  where
    helper d = (let div a b = a + b in d `div` 1) + inner d
    inner y = y `div` 2

topRem :: Int
topRem = (3 `rem` 4) + rem 1 2

builtins :: (Int, Int, Int)
builtins = (10 `div` 3, 10 `mod` 3, 10 `quot` 3)

main :: IO ()
main = do
    print (whereDiv 7 2)
    print (whereMod 7 2)
    print (whereQuot (error "boom1") 4)
    print (whereRem 7 2)
    print (whereSeq (error "boom2") 4)
    print (letDiv 100)
    print (letMod 1)
    print (letQuot 1)
    print (letRem 5)
    print (letSeq 1)
    print (applyDiv (+))
    print (applyMod (-))
    print (applyQuot (*))
    print (applyRem (\a b -> a - b))
    print (applySeq (\a _ -> a))
    print (underDiv (+) 10)
    print (underMod (+) [1, 2, 3, 4, 5])
    print (underQuot (+) [8, 9, 10])
    print (underLet 10)
    print topRem
    print builtins
