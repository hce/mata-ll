-- Sections and partial applications share a captured operand. GHC desugars
-- `(op e)` as `let y = e in \x -> x op y` and `(e op)` as `(op) e`, and a
-- partial application `f e` binds `e` once, so `filter (< maximum xs) xs`
-- computes `maximum xs` once, not per element. mata-ll spelled the operand
-- inside the section's lambda and emitted a strict partial application's
-- argument inside the closure body, so both re-evaluated it at every
-- application (seconds for a few thousand elements where GHC takes
-- milliseconds); a composition of a partial application rebuilt the
-- partial application per call. A section over an operand that is work is
-- now its lambda under a `let` binding the operand, and a closure built
-- around a captured argument that is work binds it once as a memoizing
-- suspension the body reads. Sharing never changes strictness: a section
-- or partial application over a bottom operand is a function value until
-- it is applied, and the operand is forced the first time an application
-- reads it. A point-free definition over a partial application is padded
-- to its arity and emitted as the saturated call it denotes. Shapes: the
-- review's probes (a right section over `maximum`, a strict partial
-- application, a left section, `subtract`, a composition of a partial
-- application), both backtick sections, both `$` sections, a partial
-- constructor application, a section whose operand is an infinite list
-- (shared across applications, never forced past what `elem` reads),
-- sections stored in a list and applied later, nested sections under
-- composition, a section over a constructor application, point-free
-- definitions over partial applications (a strict callee, a lazy callee,
-- a where-bound operand, a parameter-dependent operand), sections and
-- partial applications over bottoms seq'd then applied, a lazy callee's
-- partially applied bottom, a non-strict composition over a bottom
-- partial application, and the strict IORef modify over a section whose
-- operand is a call. Found by the 2026-10 fresh-eyes review (O7).
-- expect: 2999
-- expect: 105150
-- expect: 945150
-- expect: -1350404850
-- expect: 150300
-- expect: [3,6,10]
-- expect: [30,15,10]
-- expect: [6,7]
-- expect: [6,7]
-- expect: [Pair 4501500 1,Pair 4501500 2]
-- expect: [True,True,True]
-- expect: [True,False,True]
-- expect: [-4501498,-4501496]
-- expect: [True,False]
-- expect: [11,12]
-- expect: [1,2]
-- expect: [11,12]
-- expect: [4,5]
-- expect: 1
-- expect: 1
-- expect: raised at the application
-- expect: 5
-- expect: raised at the application
-- expect: 1
-- expect: 385
-- expect: 3000
import Data.IORef

add :: Int -> Int -> Int
add a b = a + b

lazyK :: Int -> Int -> Int
lazyK _ b = b

slow :: Int -> Int
slow n = go n 0
  where
    go 0 acc = acc
    go k acc = go (k - 1) (acc + 1)

data Pair = Pair Int Int deriving Show

inc :: Int -> Int
inc = add (slow 10)

incL :: Int -> Int
incL = lazyK (slow 10)

incW :: Int -> Int
incW = add (slow n) where n = 10

fx :: Int -> Int -> Int
fx x = add (slow x)

main :: IO ()
main = do
    let xs = [1 .. 3000] :: [Int]
    print (length (filter (< maximum xs) xs))
    print (sum (map (add (slow 200)) [1 .. 300]))
    print (sum (map (length xs +) [1 .. 300]))
    print (sum (map (subtract (sum xs)) [1 .. 300]))
    print (sum (map (add (slow 200) . (* 2)) [1 .. 300]))
    print (map (`div` slow 3) [10, 20, 30])
    print (map (slow 30 `div`) [1, 2, 3])
    print (map ($ slow 5) [add 1, add 2])
    print (map (add 1 $) [slow 5, slow 6])
    print (map (Pair (sum xs)) [1, 2])
    print (map (`elem` [1 ..]) [1, 2, 3])
    let fs = [(< maximum xs), (> sum xs), (== slow 7)]
    print (map (\f -> f 7) fs)
    print (map (subtract (sum xs) . (* 2)) [1, 2])
    print (map (== Just (slow 3)) [Just 3, Nothing])
    print (map inc [1, 2])
    print (map incL [1, 2])
    print (map incW [1, 2])
    print (map (fx 3) [1, 2])
    print (((+ error "boom") :: Int -> Int) `seq` (1 :: Int))
    let f = add (error "boom")
    print (f `seq` (1 :: Int))
    catch (print (f 1)) (\_ -> putStrLn "raised at the application")
    let g = lazyK (error "boom")
    print (g 5)
    catch (print ((< error "boom") (1 :: Int))) (\_ -> putStrLn "raised at the application")
    print ((const (1 :: Int) . add (error "boom")) 5)
    total <- newIORef (0 :: Integer)
    mapM_ (\i -> modifyIORef' total (+ toInteger (i * i))) [1 .. 10 :: Int]
    readIORef total >>= print
    acc <- newIORef (0 :: Int)
    modifyIORef' acc (+ length xs)
    readIORef acc >>= print
