-- A polymorphic argument reaches a higher-rank parameter only through a
-- direct application or `$`, where the parameter's quantifier is
-- skolemized and the argument checked against the rigid body. Every
-- route that goes through another function's type variable (`map`, `.`,
-- `flip`, a list, a lambda-bound variable) is rejected, as GHC rejects
-- it; those are pinned in compile_errors.rs. This case pins the legal
-- routes: direct application and `$` of `id`, a lambda, a composition and
-- another rank-2 function's parameter; a let alias and a signed alias of
-- the rank-2 function applied directly; two binders, spelled `forall a
-- b.` and `forall a. forall b.`; a rank-3 parameter met by a lambda
-- (whose binder gets the polymorphic type), a signed function, and `$`;
-- a quantified data field; runST applied directly and with `$`; a
-- let-generalized lambda. (A where-bound alias of the parameter, `g = f`,
-- is accepted too, but not pinned here: the GHC twin runs under GADTs,
-- which switches on MonoLocalBinds and leaves such a `g` monomorphic.)

applyBoth :: (forall a. a -> a) -> (Int, Bool)
applyBoth f = (f 1, f True)

viaSig :: (forall a. a -> a) -> (Int, Bool)
viaSig = applyBoth

outer :: (forall a. a -> a) -> (Int, Bool)
outer f = applyBoth f

twoBinders :: (forall a b. a -> b -> a) -> (Int, Bool)
twoBinders f = (f 1 True, f False 2)

twoNested :: (forall a. forall b. a -> b -> a) -> (Int, Bool)
twoNested f = (f 1 True, f False 2)

apTwice :: (forall a. a -> a) -> (forall b. b -> b) -> Int
apTwice f g = f (g 1)

firstOf :: (forall a. a -> a) -> Int
firstOf f = fst (applyBoth f) + 1

nest :: ((forall a. a -> a) -> Int) -> Int
nest g = g id + 1

nestPair :: ((forall a. a -> a) -> (Int, Bool)) -> Int
nestPair g = fst (g id) + 1

data Poly = Poly (forall a. a -> a)

usePoly :: Poly -> (Int, Bool)
usePoly (Poly f) = (f 1, f True)

sumST :: [Int] -> Int
sumST xs = runST (do
    acc <- newSTArray 1 0
    sumGo acc xs
    readSTArray acc 0)
  where
    sumGo acc [] = pure ()
    sumGo acc (x:rest) = do
      old <- readSTArray acc 0
      writeSTArray acc 0 (old + x)
      sumGo acc rest

sumSTDollar :: [Int] -> Int
sumSTDollar xs = runST $ do
    acc <- newSTArray 1 0
    mapM_ (\x -> do
        old <- readSTArray acc 0
        writeSTArray acc 0 (old + x)) xs
    readSTArray acc 0

main :: IO ()
main = do
    print (applyBoth id)
    print (applyBoth $ id)
    print (applyBoth (\x -> x))
    print (applyBoth (id . id))
    print (viaSig id)
    print (outer id)
    print (let g = applyBoth in g id)
    print (let g = applyBoth in g $ id)
    print (let g = \x -> x in applyBoth g)
    print (twoBinders const)
    print (twoBinders (\x _ -> x))
    print (twoNested const)
    print (apTwice id id, apTwice id $ id)
    print (nest (\f -> f 3), nest firstOf, nest $ firstOf, nest (\f -> firstOf f))
    print (nestPair (\f -> (f 3, f True)), nestPair applyBoth, nestPair $ applyBoth)
    print (usePoly (Poly id))
    print (sumST [1, 2, 3, 4, 5], sumSTDollar [1, 2, 3, 4, 5])
